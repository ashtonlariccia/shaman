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

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use shaman_core::proto::OpenRequest;
use shaman_core::{
    profiles, ConnectionStore, DiscoveredKey, Helper, KnownHosts, Pin, PinKind, PinStore,
    PtyOptions, PtySession, Registry, SavedConnection, Session, SessionId, ShellProfile, SshAuth,
    SshError, SshFailure, SshOptions, SshSession, TrustedHost,
};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, State};

#[derive(Default)]
pub struct Sessions {
    registry: Mutex<Registry>,
    /// Launched on demand: if every tab is elevated, no helper is ever needed.
    helper: Mutex<Option<Arc<Helper>>>,
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
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(1);

    let label = format!("win-{}", NEXT.fetch_add(1, Ordering::Relaxed));

    tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::default())
        .title("Shaman")
        .inner_size(1200.0, 800.0)
        .min_inner_size(720.0, 480.0)
        .decorations(false)
        .build()
        .map_err(|e| format!("could not open a new window: {e}"))?;

    tracing::info!("opened window {label}");
    Ok(label)
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

    let exit_app = app.clone();
    let on_exit = move || {
        tracing::info!(target: "shaman::pty", "SESSION_EXIT id={id}");
        let _ = exit_app.emit("session-exit", id);
    };
    let on_data = move |chunk: Vec<u8>| {
        // A dead channel just means the window went away.
        let _ = on_output.send(InvokeResponseBody::Raw(chunk));
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

    let exit_app = app.clone();
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
        SshSession::connect(
            session_id,
            opts,
            move |chunk| {
                let _ = on_output.send(InvokeResponseBody::Raw(chunk));
            },
            move || {
                tracing::info!(target: "shaman::ssh", "SESSION_EXIT id={id}");
                let _ = exit_app.emit("session-exit", id);
            },
        )
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
    // Dropping the helper closes the pipe, which ends its read loop and exits it.
    if let Ok(mut helper) = sessions.helper.lock() {
        helper.take();
    }
    tracing::info!("exiting on user request");
    app.exit(0);
}
