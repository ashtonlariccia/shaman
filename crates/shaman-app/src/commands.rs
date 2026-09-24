//! Tauri command surface for terminal sessions.
//!
//! This layer owns the registry, the IPC transport, and the decision of *where*
//! a session runs. All terminal behaviour lives in `shaman-core`.
//!
//! ## Where a session runs
//!
//! Release builds run elevated, so a shell spawned in-process would inherit
//! administrator rights. Routing is therefore:
//!
//! | app elevated | profile elevated | runs                          |
//! |--------------|------------------|-------------------------------|
//! | yes          | yes              | in-process (already admin)    |
//! | yes          | no               | in the medium-integrity helper|
//! | no           | no               | in-process (already normal)   |
//! | no           | yes              | refused, with an explanation  |
//!
//! The last row matters: if de-elevation or elevation is unavailable we fail
//! loudly. Silently handing someone an admin shell they didn't ask for — or a
//! normal one they believe is admin — is worse than an error.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use shaman_core::proto::OpenRequest;
use shaman_core::{
    profiles, ConnectionStore, DiscoveredKey, Helper, KnownHosts, Pin, PinKind, PinStore,
    PtyOptions, PtySession, Registry, SavedConnection, Session, SessionId, ShellProfile, SshAuth,
    SshError, SshFailure, SshOptions, SshSession, TrustedHost,
};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager, State, Window};

/// Where a session's output is going right now.
type Sink = Channel<InvokeResponseBody>;

/// Most a detached session may pile up before the oldest bytes are dropped.
///
/// Only ever reached if a handover stalls — the gap between one window letting
/// go of a terminal and another taking it is a few milliseconds — so this is a
/// ceiling on a pathological case, not a working buffer size.
const MAX_HELD_BYTES: usize = 256 * 1024;

/// The routing table between live sessions and the windows watching them.
///
/// A session used to write straight into the `Channel` handed over by whichever
/// window opened it, which welded the two together for life: moving a terminal
/// to another window was impossible because its output had nowhere else to go.
/// One level of indirection is what makes the drag work — the channel becomes a
/// slot that can be emptied and refilled, and a session whose slot is empty
/// keeps its bytes until someone asks for them.
#[derive(Default)]
struct Outputs {
    sinks: HashMap<u64, Sink>,
    /// Output produced while a session had no window. Replayed on attach.
    held: HashMap<u64, Vec<u8>>,
}

impl Outputs {
    fn deliver(&mut self, id: u64, chunk: Vec<u8>) {
        if let Some(sink) = self.sinks.get(&id) {
            // A dead channel just means the window went away.
            let _ = sink.send(InvokeResponseBody::Raw(chunk));
            return;
        }

        let buf = self.held.entry(id).or_default();
        buf.extend_from_slice(&chunk);
        // Dropping from the front can bisect an escape sequence, which is the
        // lesser evil: the alternative is an unbounded buffer behind a window
        // that may never arrive.
        if buf.len() > MAX_HELD_BYTES {
            buf.drain(..buf.len() - MAX_HELD_BYTES);
        }
    }

    /// Point a session at a window, handing back whatever it produced meanwhile.
    fn bind(&mut self, id: u64, sink: Sink) -> Vec<u8> {
        self.sinks.insert(id, sink);
        self.held.remove(&id).unwrap_or_default()
    }

    /// Let go of a session without losing what it says next.
    fn unbind(&mut self, id: u64) {
        self.sinks.remove(&id);
    }

    fn forget(&mut self, id: u64) {
        self.sinks.remove(&id);
        self.held.remove(&id);
    }
}

#[derive(Default)]
pub struct Sessions {
    registry: Mutex<Registry>,
    /// Launched on demand: if every tab is elevated, no helper is ever needed.
    helper: Mutex<Option<Arc<Helper>>>,
    /// Shared with every session's output thread, so it is an `Arc` rather than
    /// borrowed from the `State` the commands see.
    outputs: Arc<Mutex<Outputs>>,
    /// Terminals handed to a window that has not claimed them yet, by label.
    /// A brand-new window cannot be told anything until its page is up, so the
    /// payload waits here instead of being emitted into the void.
    handoffs: Mutex<HashMap<String, Vec<serde_json::Value>>>,
}

impl Sessions {
    fn with<T>(
        &self,
        id: u64,
        f: impl FnOnce(&mut (dyn Session + 'static)) -> shaman_core::Result<T>,
    ) -> Result<T, String> {
        let mut reg = self.registry.lock().map_err(|_| "session lock poisoned")?;
        let session = reg
            .get_mut(SessionId(id))
            .ok_or_else(|| format!("no session {id}"))?;
        f(session).map_err(|e| e.to_string())
    }

    /// The output callback for a session: routes through [`Outputs`] rather
    /// than capturing a channel, so the destination can change later.
    fn route(&self, id: u64) -> impl Fn(Vec<u8>) + Send + 'static {
        let outputs = Arc::clone(&self.outputs);
        move |chunk| {
            if let Ok(mut outputs) = outputs.lock() {
                outputs.deliver(id, chunk);
            }
        }
    }

    fn with_outputs<T>(&self, f: impl FnOnce(&mut Outputs) -> T) -> Result<T, String> {
        let mut outputs = self.outputs.lock().map_err(|_| "output lock poisoned")?;
        Ok(f(&mut outputs))
    }

    /// The helper, started if this is the first ordinary tab.
    fn helper(&self) -> Result<Arc<Helper>, String> {
        let mut slot = self.helper.lock().map_err(|_| "helper lock poisoned")?;
        if let Some(helper) = slot.as_ref() {
            return Ok(Arc::clone(helper));
        }

        let helper = Helper::launch(&helper_path()?).map_err(|e| e.to_string())?;
        *slot = Some(Arc::clone(&helper));
        tracing::info!(target: "shaman::pty", "HELPER_STARTED");
        Ok(helper)
    }
}

/// The helper ships beside the app.
fn helper_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "executable has no parent directory".to_string())?;
    Ok(dir.join("shaman-helper.exe"))
}

/// Shells that are actually installed on this machine.
#[tauri::command]
pub fn list_profiles() -> Vec<ShellProfile> {
    profiles::detect()
}

/// Open another Shaman window (File -> New Window).
///
/// A second webview in the *same* process rather than a second process: it
/// shares the WebView2 host and the session registry, which keeps the memory
/// cost of an extra window small. Labels are `win-N`, matched by the `win-*`
/// entry in `capabilities/default.json` -- without that the new window would
/// have no permissions and its menus would silently fail.
///
/// **Must be `async`.** Synchronous Tauri commands run on the main thread, and
/// window creation needs that thread to pump the event loop — so building a
/// window from a sync command deadlocks: `build()` simply never returns, and the
/// half-created window shows as a blank white pane. Marking it async moves the
/// work off the main thread.
#[tauri::command]
pub async fn new_window(app: AppHandle) -> Result<String, String> {
    build_window(&app, None)
}

/// Open a window, optionally placed so a given screen point lands on its title
/// bar -- which is where the cursor is when a terminal is dropped on the desktop.
///
/// Shared by File -> New Window and by a drag that lands on nothing, so the two
/// cannot drift apart in size, decoration or transparency.
fn build_window(app: &AppHandle, at: Option<(i32, i32)>) -> Result<String, String> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(1);

    let label = format!("win-{}", NEXT.fetch_add(1, Ordering::Relaxed));

    let mut builder = tauri::WebviewWindowBuilder::new(app, &label, tauri::WebviewUrl::default())
        .title("Shaman")
        // Matches the window in tauri.conf.json.
        .inner_size(960.0, 600.0)
        .min_inner_size(520.0, 360.0)
        .decorations(false)
        // Matches the window in tauri.conf.json. The page paints its own
        // background, so an opaque look costs nothing -- but a window built
        // opaque could never become translucent without a restart.
        .transparent(true);

    if let Some((x, y)) = at {
        // Offset so the pointer sits inside the title bar rather than on the
        // window's top-left corner: the window appears *under* the terminal you
        // just dropped, which is what the gesture implies.
        builder = builder.position(f64::from(x - 140), f64::from(y - 14));
    }

    builder
        .build()
        .map_err(|e| format!("could not open a new window: {e}"))?;

    // Effects are per-window and don't carry into a new one.
    restore_material(app);

    tracing::info!("opened window {label}");
    Ok(label)
}

// --- moving a terminal between windows --------------------------------------

/// What a drag was released over.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DropTarget {
    /// The Shaman window under the cursor, or `None` for anywhere else.
    pub label: Option<String>,
    /// The cursor itself, in physical screen pixels, for placing a new window.
    pub x: i32,
    pub y: i32,
}

/// Which window the pointer is over right now.
///
/// Asked of the OS rather than worked out in the frontend. A webview holds the
/// mouse capture for the length of a drag, so its pointer events say nothing
/// about what is underneath, and it cannot see other windows at all.
#[tauri::command]
pub fn drop_target(app: AppHandle) -> DropTarget {
    let Some((x, y)) = shaman_core::cursor_position() else {
        return DropTarget {
            label: None,
            x: 0,
            y: 0,
        };
    };

    let label = shaman_core::root_window_at(x, y).and_then(|root| {
        app.webview_windows().into_iter().find_map(|(label, win)| {
            let hwnd = win.hwnd().ok()?;
            (hwnd.0 as isize == root).then_some(label)
        })
    });

    DropTarget { label, x, y }
}

/// Hand a live terminal to another window, or to a window opened for it.
///
/// `payload` is opaque here on purpose: what a tab *is* -- its title, its kind,
/// the snapshot of its screen -- is the frontend's business, and this layer only
/// has to get it to the right window. All the backend contributes is the
/// routing and the guarantee that the session itself keeps running throughout.
#[tauri::command]
pub async fn handoff_session(
    app: AppHandle,
    sessions: State<'_, Sessions>,
    target: Option<String>,
    payload: serde_json::Value,
    x: Option<i32>,
    y: Option<i32>,
) -> Result<String, String> {
    let label = match &target {
        Some(label) => {
            if app.get_webview_window(label).is_none() {
                return Err(format!("window {label} is gone"));
            }
            label.clone()
        }
        None => {
            let at = match (x, y) {
                (Some(x), Some(y)) => Some((x, y)),
                _ => None,
            };
            build_window(&app, at)?
        }
    };

    // Queued before the window can possibly ask, so a page that comes up fast
    // cannot race past its own payload.
    sessions
        .handoffs
        .lock()
        .map_err(|_| "handoff lock poisoned")?
        .entry(label.clone())
        .or_default()
        .push(payload);

    // A window that already exists is mounted and listening; a new one claims
    // its queue when its page comes up, so there is nothing to tell it.
    if target.is_some() {
        if let Some(win) = app.get_webview_window(&label) {
            let _ = win.emit("session-adopt", ());
            let _ = win.set_focus();
        }
    }

    tracing::info!(target: "shaman::ui", "HANDOFF -> {label}");
    Ok(label)
}

/// Terminals waiting for this window, emptied as they are handed over.
#[tauri::command]
pub fn claim_handoffs(
    window: Window,
    sessions: State<'_, Sessions>,
) -> Result<Vec<serde_json::Value>, String> {
    Ok(sessions
        .handoffs
        .lock()
        .map_err(|_| "handoff lock poisoned")?
        .remove(window.label())
        .unwrap_or_default())
}

/// Stop delivering a session's output; hold it instead.
///
/// The session keeps running -- this is the first half of a move, not a close.
#[tauri::command]
pub fn session_detach(sessions: State<'_, Sessions>, id: u64) -> Result<(), String> {
    sessions.with_outputs(|outputs| outputs.unbind(id))?;
    tracing::info!(target: "shaman::pty", "SESSION_DETACH id={id}");
    Ok(())
}

/// Take over a running session, handing back whatever it said while detached.
///
/// The backlog comes back as the command's own result rather than down the
/// channel, so the caller can paint its restored screen first and apply what
/// came after it second. Two separate deliveries could arrive either way round.
#[tauri::command]
pub fn session_attach(
    sessions: State<'_, Sessions>,
    on_output: Channel<InvokeResponseBody>,
    id: u64,
) -> Result<tauri::ipc::Response, String> {
    {
        let mut reg = sessions
            .registry
            .lock()
            .map_err(|_| "session lock poisoned")?;
        if reg.get_mut(SessionId(id)).is_none() {
            return Err(format!("no session {id}"));
        }
    }

    let held = sessions.with_outputs(|outputs| outputs.bind(id, on_output))?;
    tracing::info!(
        target: "shaman::pty",
        "SESSION_ATTACH id={id} backlog={}",
        held.len()
    );
    Ok(tauri::ipc::Response::new(held))
}

/// Open a shell session and stream its output over `on_output`.
///
/// Output crosses the boundary as `Raw` bytes rather than JSON, so a terminal
/// flood does not pay for base64 or per-byte number arrays.
#[tauri::command]
pub fn session_open(
    app: AppHandle,
    sessions: State<'_, Sessions>,
    on_output: Channel<InvokeResponseBody>,
    profile_id: String,
    cols: u16,
    rows: u16,
) -> Result<u64, String> {
    // Re-detect rather than trusting the id: the frontend's list could be stale
    // if a shell was uninstalled since the menu was populated.
    let profile = profiles::find(&profile_id)
        .ok_or_else(|| format!("unknown or no-longer-installed shell '{profile_id}'"))?;

    let session_id = SessionId::next();
    let id = session_id.0;
    let elevated_app = shaman_core::is_elevated();

    // The window that opened the tab is only its *first* home -- a terminal can
    // be dragged to another one later -- so output goes through the routing
    // table rather than straight down this channel.
    sessions.with_outputs(|outputs| outputs.bind(id, on_output))?;
    let on_data = sessions.route(id);

    let exit_app = app.clone();
    let exit_outputs = Arc::clone(&sessions.outputs);
    let on_exit = move || {
        if let Ok(mut outputs) = exit_outputs.lock() {
            outputs.forget(id);
        }
        tracing::info!(target: "shaman::pty", "SESSION_EXIT id={id}");
        let _ = exit_app.emit("session-exit", id);
    };

    let cols = cols.max(1);
    let rows = rows.max(1);

    let session: Box<dyn Session> = if profile.elevated {
        if !elevated_app {
            return Err(
                "Admin terminals need Shaman itself to be running as administrator. \
                 Restart Shaman and accept the UAC prompt."
                    .into(),
            );
        }
        // Already elevated, so an in-process ConPTY is an admin shell.
        Box::new(
            PtySession::spawn(
                session_id,
                PtyOptions::from_profile(&profile, cols, rows),
                on_data,
                on_exit,
            )
            .map_err(|e| format!("failed to start shell: {e}"))?,
        )
    } else if elevated_app {
        // Ordinary tab while we are elevated: it must be pushed back down to
        // normal integrity, which only the helper can do.
        let helper = sessions
            .helper()
            .map_err(|e| format!("cannot open a normal terminal without the helper: {e}"))?;

        let request = OpenRequest {
            program: profile.program.clone(),
            args: profile.args.clone(),
            cwd: None,
            cols,
            rows,
        };

        Box::new(
            helper
                .open(session_id, profile.kind.clone(), request, on_data, on_exit)
                .map_err(|e| format!("helper refused the session: {e}"))?,
        )
    } else {
        // Not elevated, ordinary tab: nothing to do, spawn it directly.
        Box::new(
            PtySession::spawn(
                session_id,
                PtyOptions::from_profile(&profile, cols, rows),
                on_data,
                on_exit,
            )
            .map_err(|e| format!("failed to start shell: {e}"))?,
        )
    };

    sessions
        .registry
        .lock()
        .map_err(|_| "session lock poisoned")?
        .insert(session);

    tracing::info!(
        target: "shaman::pty",
        "SESSION_OPEN id={id} profile={profile_id} elevated={} via={} {cols}x{rows}",
        profile.elevated,
        if profile.elevated || !elevated_app { "in-process" } else { "helper" }
    );
    Ok(id)
}

/// Open an SSH session.
///
/// Errors come back as a structured [`SshError`] rather than a string so the
/// connect dialog can tell a rejected password (retry here) from an unreachable
/// host (fix the address) without parsing prose.
///
/// **Must be `async`, and the connect must run on a blocking task.**
/// `SshSession::connect` waits for the worker thread to report success or
/// failure, which for an unreachable host means waiting out a timeout. A
/// synchronous Tauri command runs on the main thread — the same trap documented
/// on [`new_window`] — so doing that wait there freezes the entire window:
/// menus, other tabs, even the close button. Reaching a dead host must cost one
/// tab its progress spinner, not the whole application.
/// Where to connect and as whom. Grouped into a struct rather than four loose
/// parameters -- it keeps the command signature readable and the password
/// travelling alongside the identity it belongs to.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshTarget {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: SshAuth,
    /// Set only after the user has been shown the fingerprint and accepted it.
    #[serde(default)]
    pub trust_new_key: bool,
}

#[tauri::command]
pub async fn ssh_connect(
    app: AppHandle,
    sessions: State<'_, Sessions>,
    on_output: Channel<InvokeResponseBody>,
    target: SshTarget,
    cols: u16,
    rows: u16,
) -> Result<u64, SshError> {
    let SshTarget {
        host,
        port,
        username,
        auth,
        trust_new_key,
    } = target;

    let session_id = SessionId::next();
    let id = session_id.0;

    sessions
        .with_outputs(|outputs| outputs.bind(id, on_output))
        .map_err(|e| SshError {
            kind: SshFailure::Session,
            message: e,
            fingerprint: None,
            expected_fingerprint: None,
        })?;
    let on_data = sessions.route(id);

    let exit_app = app.clone();
    let exit_outputs = Arc::clone(&sessions.outputs);
    let opts = SshOptions {
        host: host.trim().to_string(),
        port: if port == 0 { 22 } else { port },
        username: username.trim().to_string(),
        auth,
        cols: cols.max(1),
        rows: rows.max(1),
        trust_new_key,
    };
    let label = opts.label();

    let session = tauri::async_runtime::spawn_blocking(move || {
        SshSession::connect(session_id, opts, on_data, move || {
            if let Ok(mut outputs) = exit_outputs.lock() {
                outputs.forget(id);
            }
            tracing::info!(target: "shaman::ssh", "SESSION_EXIT id={id}");
            let _ = exit_app.emit("session-exit", id);
        })
    })
    .await
    .map_err(|e| SshError {
        kind: SshFailure::Session,
        message: format!("the connection task did not finish: {e}"),
        fingerprint: None,
        expected_fingerprint: None,
    })??;

    sessions
        .registry
        .lock()
        .map_err(|_| SshError {
            kind: SshFailure::Session,
            message: "session lock poisoned".into(),
            fingerprint: None,
            expected_fingerprint: None,
        })?
        .insert(Box::new(session));

    tracing::info!(target: "shaman::ssh", "SSH_OPEN id={id} {label} {cols}x{rows}");
    Ok(id)
}

// --- saved connections ------------------------------------------------------
//
// Passwords never cross the IPC boundary. The UI deals in ids; the secret is
// decrypted here, used, and dropped.

/// Private keys found in the user's `.ssh` directory, for the connect dialog.
#[tauri::command]
pub fn ssh_keys() -> Vec<DiscoveredKey> {
    shaman_core::ssh::discover_keys()
}

/// Saved connections, without any secret material.
#[tauri::command]
pub fn saved_connections() -> Result<Vec<SavedConnection>, String> {
    Ok(ConnectionStore::load().map_err(|e| e.to_string())?.list())
}

/// Whether to offer to save after connecting to an unrecognised host.
#[tauri::command]
pub fn suggest_saving_enabled() -> Result<bool, String> {
    Ok(ConnectionStore::load()
        .map_err(|e| e.to_string())?
        .suggest_saving)
}

/// "Never show again" from the save prompt.
#[tauri::command]
pub fn set_suggest_saving(enabled: bool) -> Result<(), String> {
    let mut store = ConnectionStore::load().map_err(|e| e.to_string())?;
    store.suggest_saving = enabled;
    store.save().map_err(|e| e.to_string())
}

/// Is this target already saved? Drives whether the prompt appears at all.
#[tauri::command]
pub fn connection_is_saved(host: String, port: u16, username: String) -> Result<bool, String> {
    Ok(ConnectionStore::load()
        .map_err(|e| e.to_string())?
        .find(&host, port, &username)
        .is_some())
}

#[tauri::command]
pub fn save_connection(target: SshTarget) -> Result<SavedConnection, String> {
    let mut store = ConnectionStore::load().map_err(|e| e.to_string())?;
    let saved = store
        .upsert(
            target.host.trim(),
            if target.port == 0 { 22 } else { target.port },
            target.username.trim(),
            &target.auth,
        )
        .map_err(|e| e.to_string())?;
    store.save().map_err(|e| e.to_string())?;

    tracing::info!(target: "shaman::ssh", "CONNECTION_SAVED {}", saved.label());
    Ok(saved)
}

/// Set or clear a saved connection's display name.
#[tauri::command]
pub fn rename_connection(id: String, name: String) -> Result<SavedConnection, String> {
    let mut store = ConnectionStore::load().map_err(|e| e.to_string())?;
    let renamed = store.rename(&id, &name).map_err(|e| e.to_string())?;
    store.save().map_err(|e| e.to_string())?;
    Ok(renamed)
}

#[tauri::command]
pub fn remove_connection(id: String) -> Result<(), String> {
    let mut store = ConnectionStore::load().map_err(|e| e.to_string())?;
    if !store.remove(&id) {
        return Err(format!("no saved connection {id}"));
    }
    store.save().map_err(|e| e.to_string())?;

    // A pin for a connection that no longer exists is a button that can't open
    // anything, so removal takes the pin with it.
    let mut pins = PinStore::load().map_err(|e| e.to_string())?;
    if pins.forget_connection(&id) {
        pins.save().map_err(|e| e.to_string())?;
        tracing::info!(target: "shaman::pins", "PIN_DROPPED_WITH_CONNECTION {id}");
    }

    tracing::info!(target: "shaman::ssh", "CONNECTION_REMOVED {id}");
    Ok(())
}

// --- pinned connections -----------------------------------------------------
//
// The quick-open strip along the bottom of the window. Every command returns the
// whole strip, so the UI never has to guess what changed.

#[tauri::command]
pub fn pinned_connections() -> Result<Vec<Pin>, String> {
    Ok(PinStore::load().map_err(|e| e.to_string())?.list())
}

/// Pin a shell profile or a saved connection.
///
/// Pinning something already pinned is not an error — it refreshes the stored
/// label and leaves the strip's length alone.
#[tauri::command]
pub fn pin_connection(pin: Pin) -> Result<Vec<Pin>, String> {
    let mut store = PinStore::load().map_err(|e| e.to_string())?;
    let added = store.add(pin.clone());
    store.save().map_err(|e| e.to_string())?;

    if added {
        tracing::info!(target: "shaman::pins", "PINNED {:?} {}", pin.kind, pin.target);
    }
    Ok(store.list())
}

/// Drop a pin at a new position on the strip (drag to reorder).
///
/// `index` counts in the list as it will be *after* the move, which is what the
/// drag already knows: the gap it was released over.
#[tauri::command]
pub fn move_pin(kind: PinKind, target: String, index: usize) -> Result<Vec<Pin>, String> {
    let mut store = PinStore::load().map_err(|e| e.to_string())?;
    if store.move_to(kind, &target, index) {
        store.save().map_err(|e| e.to_string())?;
        tracing::info!(target: "shaman::pins", "PIN_MOVED {kind:?} {target} -> {index}");
    }
    Ok(store.list())
}

#[tauri::command]
pub fn unpin_connection(kind: PinKind, target: String) -> Result<Vec<Pin>, String> {
    let mut store = PinStore::load().map_err(|e| e.to_string())?;
    if store.remove(kind, &target) {
        store.save().map_err(|e| e.to_string())?;
        tracing::info!(target: "shaman::pins", "UNPINNED {kind:?} {target}");
    }
    Ok(store.list())
}

// --- trusted host keys ------------------------------------------------------

/// Every host key Shaman currently trusts.
#[tauri::command]
pub fn trusted_hosts() -> Result<Vec<TrustedHost>, String> {
    Ok(KnownHosts::load().map_err(|e| e.to_string())?.list())
}

/// Forget a trusted key. The next connection to that host will ask again.
#[tauri::command]
pub fn forget_trusted_host(key: String) -> Result<(), String> {
    let mut known = KnownHosts::load().map_err(|e| e.to_string())?;
    if !known.forget_key(&key) {
        return Err(format!("no trusted host {key}"));
    }
    known.save().map_err(|e| e.to_string())?;
    tracing::info!(target: "shaman::ssh", "HOST_KEY_FORGOTTEN {key}");
    Ok(())
}

/// Connect using a saved connection.
///
/// The password is read and decrypted here rather than handed to the frontend
/// first — autofill should not mean "send the secret to the UI and back".
#[tauri::command]
pub async fn ssh_connect_saved(
    app: AppHandle,
    sessions: State<'_, Sessions>,
    on_output: Channel<InvokeResponseBody>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<u64, SshError> {
    let store = ConnectionStore::load().map_err(|e| SshError {
        kind: SshFailure::Session,
        message: e.to_string(),
        fingerprint: None,
        expected_fingerprint: None,
    })?;

    let saved = store.get(&id).cloned().ok_or_else(|| SshError {
        kind: SshFailure::Session,
        message: format!("no saved connection {id}"),
        fingerprint: None,
        expected_fingerprint: None,
    })?;

    let auth = store.auth(&id).map_err(|e| SshError {
        kind: SshFailure::Auth,
        message: format!("could not read the saved credentials: {e}"),
        fingerprint: None,
        expected_fingerprint: None,
    })?;

    ssh_connect(
        app,
        sessions,
        on_output,
        SshTarget {
            host: saved.host,
            port: saved.port,
            username: saved.username,
            auth,
            // A saved connection was verified when it was first accepted; a key
            // change must still stop us.
            trust_new_key: false,
        },
        cols,
        rows,
    )
    .await
}

/// Forward user input. `data` is what xterm.js produced, escape sequences and
/// all, so it is passed through verbatim.
#[tauri::command]
pub fn session_write(sessions: State<'_, Sessions>, id: u64, data: String) -> Result<(), String> {
    sessions.with(id, |s| s.write(data.as_bytes()))
}

#[tauri::command]
pub fn session_resize(
    sessions: State<'_, Sessions>,
    id: u64,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    sessions.with(id, |s| s.resize(cols.max(1), rows.max(1)))
}

/// Terminate a session and everything it spawned, then forget it.
#[tauri::command]
pub fn session_close(sessions: State<'_, Sessions>, id: u64) -> Result<(), String> {
    let mut reg = sessions
        .registry
        .lock()
        .map_err(|_| "session lock poisoned")?;

    if let Some(mut session) = reg.remove(SessionId(id)) {
        // Best effort: even if the kill call fails, dropping the session closes
        // its job object, which terminates the process tree anyway.
        if let Err(err) = session.kill() {
            tracing::warn!(target: "shaman::pty", "kill for id={id} failed: {err}");
        }
        tracing::info!(target: "shaman::pty", "SESSION_CLOSE id={id}");
    }
    drop(reg);

    sessions.with_outputs(|outputs| outputs.forget(id))?;
    Ok(())
}

/// Quit the application (File -> Exit).
///
/// Sessions are dropped on the way out, and each drop closes its job object,
/// so no shell survives the app.
#[tauri::command]
pub fn quit_app(app: AppHandle, sessions: State<'_, Sessions>) {
    if let Ok(mut reg) = sessions.registry.lock() {
        let ids: Vec<_> = reg.list().into_iter().map(|s| s.id).collect();
        for id in ids {
            if let Some(mut session) = reg.remove(id) {
                let _ = session.kill();
            }
        }
    }
    if let Ok(mut outputs) = sessions.outputs.lock() {
        *outputs = Outputs::default();
    }
    // Dropping the helper closes the pipe, which ends its read loop and exits it.
    if let Ok(mut helper) = sessions.helper.lock() {
        helper.take();
    }
    tracing::info!("exiting on user request");
    app.exit(0);
}

// --- appearance -------------------------------------------------------------
//
// Font, cursor and window material, shared by every terminal. Two halves: the
// values the frontend reads to configure xterm and the CSS, and the window
// effect, which only the backend can apply.

/// Apply the window material to every open window.
///
/// Acrylic is a property of the OS window, not of the page inside it, so it
/// cannot be done from CSS. Applied to all windows rather than the calling one:
/// the appearance is global, and a second window left opaque while the first
/// went frosted would look like a bug.
fn apply_material(app: &AppHandle, appearance: &shaman_core::Appearance) {
    use tauri::window::{Effect, EffectsBuilder};
    use tauri::Manager;

    let effects = match appearance.material {
        shaman_core::Material::Acrylic => {
            Some(EffectsBuilder::new().effect(Effect::Acrylic).build())
        }
        // `None` clears whatever was applied before, so turning the toggle off
        // actually removes the blur rather than leaving it stuck on.
        shaman_core::Material::None => None,
    };

    for (label, window) in app.webview_windows() {
        if let Err(e) = window.set_effects(effects.clone()) {
            // Not fatal: the opacity half still works, and an unsupported
            // material should cost the blur, not the settings dialog.
            tracing::warn!(target: "shaman::ui", "set_effects on {label} failed: {e}");
        }
    }
}

#[tauri::command]
pub fn appearance() -> Result<shaman_core::Appearance, String> {
    shaman_core::settings::load().map_err(|e| e.to_string())
}

/// Store the appearance and apply the parts the backend owns.
///
/// Returns what was actually written: the store clamps font size and opacity,
/// so the dialog must render the stored value rather than the one it sent.
#[tauri::command]
pub fn set_appearance(
    app: AppHandle,
    appearance: shaman_core::Appearance,
) -> Result<shaman_core::Appearance, String> {
    let stored = shaman_core::settings::save(&appearance).map_err(|e| e.to_string())?;
    apply_material(&app, &stored);

    tracing::info!(
        target: "shaman::ui",
        "APPEARANCE font={:?} {}px cursor={:?} opacity={} material={:?}",
        stored.font_family,
        stored.font_size,
        stored.cursor_shape,
        stored.background_opacity,
        stored.material,
    );
    Ok(stored)
}

/// Re-apply the stored material to a window that has just been created.
///
/// Window effects are per-window and do not survive into a new one, so File →
/// New Window would otherwise open opaque while the first window stayed frosted.
pub fn restore_material(app: &AppHandle) {
    match shaman_core::settings::load() {
        Ok(appearance) => apply_material(app, &appearance),
        Err(e) => tracing::warn!(target: "shaman::ui", "could not read settings: {e}"),
    }
}
