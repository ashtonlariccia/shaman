//! SSH sessions.
//!
//! A real SSH client (`russh`) rather than a wrapper around `ssh.exe`. That
//! matters for credentials: the password goes through the actual auth protocol,
//! so "did it work" is an unambiguous answer from the server rather than
//! something inferred by watching a pty for the word `password:` — which breaks
//! on non-English servers, custom prompts, banners and MFA.
//!
//! `russh` is async and [`Session`] is not, so each connection owns a thread
//! running a small single-threaded runtime. Commands go in over a channel and
//! output comes back through the same `on_data` callback a local shell uses,
//! which is what lets an SSH tab behave exactly like any other tab.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use russh::client::{self, Handler};
use russh::{ChannelMsg, Disconnect};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

use crate::known_hosts::{KnownHosts, Verdict};
use crate::pump::{self, CoalesceConfig};
use crate::session::{Session, SessionId, SessionKind};
use crate::{Error, Result};

/// How long to wait for the TCP connection itself.
///
/// Deliberately short, and checked before any SSH work happens. The everyday
/// failure is a saved device that is off, asleep, or on another network — and
/// such a host *swallows* the SYN rather than refusing it, so Windows would
/// otherwise retry for ~20s before admitting defeat. This is the "is anything
/// there at all" budget.
const REACH_TIMEOUT: Duration = Duration::from_secs(5);

/// How long the SSH handshake (banner exchange plus key exchange) may take once
/// the socket is up. Generous: this is a live host doing real work.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

/// How long authentication may take.
///
/// Without this a server that completes the handshake and then goes quiet is
/// held open by russh's hour-long inactivity timeout, and the tab sits on
/// "Connecting…" with nothing to show for it.
const AUTH_TIMEOUT: Duration = Duration::from_secs(20);

/// How to prove who we are.
///
/// Tagged rather than "password plus optional key path" so impossible states —
/// a key auth carrying a password, say — cannot be represented.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SshAuth {
    Password {
        password: String,
    },
    Key {
        /// Path to the *private* key.
        path: String,
        /// Empty when the key is not encrypted.
        #[serde(default)]
        passphrase: String,
    },
}

impl SshAuth {
    pub fn is_key(&self) -> bool {
        matches!(self, SshAuth::Key { .. })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshOptions {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: SshAuth,
    pub cols: u16,
    pub rows: u16,
    /// Accept and remember whatever key the server presents.
    ///
    /// Only ever set after a human has been shown the fingerprint and agreed.
    #[serde(default)]
    pub trust_new_key: bool,
}

impl SshOptions {
    pub fn label(&self) -> String {
        format!("{}@{}", self.username, self.host)
    }
}

/// A private key found in the user's `.ssh` directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredKey {
    pub path: String,
    /// Filename, for the picker.
    pub name: String,
    /// Encrypted keys need a passphrase, so the dialog can say so up front.
    pub encrypted: bool,
}

/// Private keys in `%USERPROFILE%\.ssh`, newest-looking names first.
///
/// Only files that actually parse as private keys are returned — listing a
/// `known_hosts` or a stray `.pub` in the picker would be worse than useless.
pub fn discover_keys() -> Vec<DiscoveredKey> {
    let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) else {
        return Vec::new();
    };
    let dir = std::path::Path::new(&home).join(".ssh");

    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        // Skip public keys and the obvious non-keys.
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".pub") || name == "known_hosts" || name == "config" {
            continue;
        }

        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if !text.contains("PRIVATE KEY") {
            continue;
        }

        // Probing with no passphrase tells us whether one will be needed.
        let encrypted = russh::keys::decode_secret_key(&text, None).is_err();

        found.push(DiscoveredKey {
            path: path.to_string_lossy().into_owned(),
            name,
            encrypted,
        });
    }

    // ed25519 first: it is what modern hosts prefer.
    found.sort_by_key(|k| match () {
        _ if k.name.contains("ed25519") => 0,
        _ if k.name.contains("ecdsa") => 1,
        _ if k.name.contains("rsa") => 2,
        _ => 3,
    });
    found
}

/// Why a connection attempt failed, in terms a user can act on.
///
/// Kept separate from the message text so the UI can treat a wrong password
/// differently from an unreachable host — the first means "try again", the
/// second means "check the address".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SshFailure {
    /// Server rejected the credentials.
    Auth,
    /// Never got a usable connection: DNS, refused, timed out, not SSH.
    Unreachable,
    /// Connected and authenticated, but the shell could not be started.
    Session,
    /// First time seeing this host; the fingerprint needs confirming.
    UnknownHostKey,
    /// The host key differs from the one previously trusted.
    HostKeyChanged,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshError {
    pub kind: SshFailure,
    pub message: String,
    /// The key the server actually presented, for the host-key prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    /// The key we had previously trusted, when it changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_fingerprint: Option<String>,
}

impl SshError {
    fn new(kind: SshFailure, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            fingerprint: None,
            expected_fingerprint: None,
        }
    }
}

impl std::fmt::Display for SshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Enforces the trust-on-first-use policy from [`crate::known_hosts`].
///
/// `check_server_key` can only answer yes/no, so when it says no it also records
/// *why* here; `establish` reads that afterwards to build a precise error
/// carrying the fingerprint the UI needs to show.
struct VerifyHostKey {
    host: String,
    port: u16,
    trust_new_key: bool,
    outcome: Arc<Mutex<Option<HostKeyOutcome>>>,
}

#[derive(Debug, Clone)]
struct HostKeyOutcome {
    fingerprint: String,
    verdict: Verdict,
}

impl Handler for VerifyHostKey {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &russh::keys::ssh_key::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        let fingerprint = key.fingerprint(russh::keys::HashAlg::Sha256).to_string();

        let known = KnownHosts::load().unwrap_or_default();
        let verdict = known.verdict(&self.host, self.port, &fingerprint);

        if let Ok(mut slot) = self.outcome.lock() {
            *slot = Some(HostKeyOutcome {
                fingerprint: fingerprint.clone(),
                verdict: verdict.clone(),
            });
        }

        match verdict {
            Verdict::Match => Ok(true),
            // The user has already been shown this fingerprint and agreed.
            Verdict::Unknown | Verdict::Changed { .. } if self.trust_new_key => {
                let mut known = known;
                known.trust(&self.host, self.port, &fingerprint);
                if let Err(e) = known.save() {
                    tracing::error!("could not record the host key: {e}");
                }
                Ok(true)
            }
            // Refuse, and let `establish` explain which case this was.
            _ => Ok(false),
        }
    }
}

enum Command {
    Data(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    Close,
}

pub struct SshSession {
    id: SessionId,
    kind: SessionKind,
    commands: UnboundedSender<Command>,
}

impl SshSession {
    /// Connect, authenticate, and start a remote shell.
    ///
    /// Blocks until the shell is running or the attempt fails, so the caller can
    /// report a precise error instead of opening an empty terminal that quietly
    /// never connects.
    pub fn connect<D, E>(
        id: SessionId,
        opts: SshOptions,
        on_data: D,
        on_exit: E,
    ) -> std::result::Result<Self, SshError>
    where
        D: Fn(Vec<u8>) + Send + 'static,
        E: FnOnce() + Send + 'static,
    {
        let (commands, mut command_rx) = unbounded_channel::<Command>();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<std::result::Result<(), SshError>>();

        let kind = SessionKind::Ssh {
            profile: opts.label(),
        };
        let thread_opts = opts.clone();

        thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = ready_tx.send(Err(SshError::new(
                        SshFailure::Session,
                        format!("could not start the SSH runtime: {e}"),
                    )));
                    return;
                }
            };

            runtime.block_on(async move {
                // Coalesce SSH output the same way PTY output is coalesced: a
                // remote `cat` of a large file is just as capable of flooding
                // the IPC channel as a local one.
                let (raw_tx, raw_rx) = std::sync::mpsc::channel::<Vec<u8>>();
                pump::spawn(raw_rx, CoalesceConfig::default(), on_data);

                // `handle` is held for the whole loop on purpose: it owns the
                // connection, and dropping it would close the channel.
                let (handle, mut channel) = match establish(&thread_opts).await {
                    Ok(pair) => {
                        let _ = ready_tx.send(Ok(()));
                        pair
                    }
                    Err(err) => {
                        let _ = ready_tx.send(Err(err));
                        return;
                    }
                };

                loop {
                    tokio::select! {
                        // Input from the UI.
                        command = command_rx.recv() => {
                            match command {
                                Some(Command::Data(bytes)) => {
                                    if channel.data(&bytes[..]).await.is_err() {
                                        break;
                                    }
                                }
                                Some(Command::Resize { cols, rows }) => {
                                    let _ = channel
                                        .window_change(cols as u32, rows as u32, 0, 0)
                                        .await;
                                }
                                // Sender dropped, or an explicit kill.
                                Some(Command::Close) | None => break,
                            }
                        }

                        // Output from the server.
                        message = channel.wait() => {
                            match message {
                                Some(ChannelMsg::Data { data }) => {
                                    if raw_tx.send(data.to_vec()).is_err() {
                                        break;
                                    }
                                }
                                // stderr from the remote shell belongs on screen too.
                                Some(ChannelMsg::ExtendedData { data, .. }) => {
                                    if raw_tx.send(data.to_vec()).is_err() {
                                        break;
                                    }
                                }
                                Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                                Some(_) => {}
                            }
                        }
                    }
                }

                let _ = channel.close().await;
                // Say goodbye properly rather than dropping the socket.
                let _ = handle.disconnect(Disconnect::ByApplication, "", "en").await;
                drop(raw_tx); // flushes the coalescer
                on_exit();
            });
        });

        // Propagate the connect result to the caller.
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self { id, kind, commands }),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(SshError::new(
                SshFailure::Session,
                "the SSH worker stopped before reporting a result",
            )),
        }
    }
}

/// Connect, authenticate, and open a shell channel.
/// Returns the handle as well as the channel: the handle owns the connection,
/// and dropping it would close the channel out from under us.
async fn establish(
    opts: &SshOptions,
) -> std::result::Result<(client::Handle<VerifyHostKey>, russh::Channel<client::Msg>), SshError> {
    let config = Arc::new(client::Config {
        inactivity_timeout: Some(Duration::from_secs(3600)),
        ..Default::default()
    });

    // Reach the host before attempting any SSH.
    //
    // This is the check that makes an out-of-network device fail in seconds
    // with a plain answer rather than stalling. Connecting the socket here
    // rather than letting russh do it is what keeps it free: the very same
    // stream is handed straight to the handshake, so a reachable host pays for
    // exactly one TCP connection, as before.
    let stream = match tokio::time::timeout(
        REACH_TIMEOUT,
        tokio::net::TcpStream::connect((opts.host.as_str(), opts.port)),
    )
    .await
    {
        Ok(Ok(stream)) => stream,
        // Refused, no route, or a name that doesn't resolve — all of which the
        // OS answers immediately and precisely, so pass the reason through.
        Ok(Err(err)) => {
            return Err(SshError::new(
                SshFailure::Unreachable,
                format!("could not reach {}:{} — {err}", opts.host, opts.port),
            ))
        }
        Err(_) => {
            return Err(SshError::new(
                SshFailure::Unreachable,
                format!(
                    "{}:{} did not answer within {}s — the device is off, on another \
                     network, or blocked by a firewall",
                    opts.host,
                    opts.port,
                    REACH_TIMEOUT.as_secs()
                ),
            ))
        }
    };

    // What `client::connect` would have done for us. Interactive typing is
    // latency-bound, so Nagle's algorithm is felt keystroke by keystroke.
    if config.nodelay {
        let _ = stream.set_nodelay(true);
    }

    // The handler records what it saw here, so a rejection can be explained
    // precisely instead of surfacing as a generic connection failure.
    let outcome: Arc<Mutex<Option<HostKeyOutcome>>> = Arc::new(Mutex::new(None));
    let handler = VerifyHostKey {
        host: opts.host.clone(),
        port: opts.port,
        trust_new_key: opts.trust_new_key,
        outcome: Arc::clone(&outcome),
    };

    let connected = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        client::connect_stream(config, stream, handler),
    )
    .await;

    let mut handle = match connected {
        Ok(Ok(handle)) => handle,
        Ok(Err(err)) => {
            // A rejected host key surfaces here as an ordinary connection
            // error, so check what the handler recorded before blaming the
            // network.
            let seen = outcome.lock().ok().and_then(|o| o.clone());
            return Err(match seen {
                Some(HostKeyOutcome {
                    fingerprint,
                    verdict: Verdict::Unknown,
                }) => SshError {
                    kind: SshFailure::UnknownHostKey,
                    message: format!(
                        "{}:{} presented a host key Shaman has not seen before.",
                        opts.host, opts.port
                    ),
                    fingerprint: Some(fingerprint),
                    expected_fingerprint: None,
                },
                Some(HostKeyOutcome {
                    fingerprint,
                    verdict: Verdict::Changed { expected },
                }) => SshError {
                    kind: SshFailure::HostKeyChanged,
                    message: format!(
                        "The host key for {}:{} has CHANGED. This happens when a server is \
                             rebuilt — but it is also what an impersonation attack looks like. \
                             Do not continue unless you know why it changed.",
                        opts.host, opts.port
                    ),
                    fingerprint: Some(fingerprint),
                    expected_fingerprint: Some(expected),
                },
                // The socket opened, so the address is right and something is
                // listening — it just isn't talking SSH, or not to us.
                _ => SshError::new(
                    SshFailure::Unreachable,
                    format!(
                        "{}:{} accepted the connection but the SSH handshake failed — {err}",
                        opts.host, opts.port
                    ),
                ),
            });
        }
        Err(_) => {
            return Err(SshError::new(
                SshFailure::Unreachable,
                format!(
                    "{}:{} accepted the connection but never completed the SSH handshake \
                     (waited {}s) — check that the port really is SSH",
                    opts.host,
                    opts.port,
                    HANDSHAKE_TIMEOUT.as_secs()
                ),
            ))
        }
    };

    let auth = match tokio::time::timeout(AUTH_TIMEOUT, authenticate(&mut handle, opts)).await {
        Ok(result) => result?,
        Err(_) => {
            return Err(SshError::new(
                SshFailure::Auth,
                format!(
                    "{}@{} never answered the login attempt (waited {}s)",
                    opts.username,
                    opts.host,
                    AUTH_TIMEOUT.as_secs()
                ),
            ))
        }
    };

    if !auth.success() {
        // The common case, and the one the dialog is built around: let the user
        // correct their credentials without losing the rest of the form.
        let detail = match &opts.auth {
            SshAuth::Password { .. } => "check the username and password",
            SshAuth::Key { .. } => "the server rejected this key for that user",
        };
        return Err(SshError::new(
            SshFailure::Auth,
            format!(
                "authentication failed for {}@{} — {detail}",
                opts.username, opts.host
            ),
        ));
    }

    let channel = handle.channel_open_session().await.map_err(|err| {
        SshError::new(
            SshFailure::Session,
            format!("connected, but could not open a session: {err}"),
        )
    })?;

    channel
        .request_pty(
            true,
            "xterm-256color",
            opts.cols.max(1) as u32,
            opts.rows.max(1) as u32,
            0,
            0,
            &[],
        )
        .await
        .map_err(|err| {
            SshError::new(
                SshFailure::Session,
                format!("the server refused a terminal: {err}"),
            )
        })?;

    channel.request_shell(true).await.map_err(|err| {
        SshError::new(
            SshFailure::Session,
            format!("the server refused a shell: {err}"),
        )
    })?;

    Ok((handle, channel))
}

/// Prove who we are, by whichever method the connection was configured with.
///
/// Split out of [`establish`] so the whole attempt can be given a deadline: a
/// server that accepts the connection and then goes silent must not park the
/// session forever.
async fn authenticate(
    handle: &mut client::Handle<VerifyHostKey>,
    opts: &SshOptions,
) -> std::result::Result<client::AuthResult, SshError> {
    Ok(match &opts.auth {
        SshAuth::Password { password } => handle
            .authenticate_password(opts.username.clone(), password.clone())
            .await
            .map_err(|err| {
                SshError::new(SshFailure::Auth, format!("authentication failed: {err}"))
            })?,

        SshAuth::Key { path, passphrase } => {
            let passphrase = (!passphrase.is_empty()).then_some(passphrase.as_str());

            // Loading is a separate failure from the server rejecting us: a
            // wrong passphrase or an unreadable file should say so, not claim
            // the server refused the key.
            let key = russh::keys::load_secret_key(path, passphrase).map_err(|err| {
                let hint = if passphrase.is_none() {
                    " — if the key is encrypted, enter its passphrase"
                } else {
                    " — check the passphrase"
                };
                SshError::new(
                    SshFailure::Auth,
                    format!("could not read the private key {path}: {err}{hint}"),
                )
            })?;

            handle
                .authenticate_publickey(
                    opts.username.clone(),
                    russh::keys::PrivateKeyWithHashAlg::new(
                        Arc::new(key),
                        // Let the server pick; modern hosts reject ssh-rsa/SHA-1.
                        handle
                            .best_supported_rsa_hash()
                            .await
                            .ok()
                            .flatten()
                            .flatten(),
                    ),
                )
                .await
                .map_err(|err| {
                    SshError::new(SshFailure::Auth, format!("authentication failed: {err}"))
                })?
        }
    })
}

impl Session for SshSession {
    fn id(&self) -> SessionId {
        self.id
    }

    fn kind(&self) -> &SessionKind {
        &self.kind
    }

    fn elevated(&self) -> bool {
        // Privilege on the remote host is the remote host's business; this flag
        // describes local Windows integrity.
        false
    }

    fn write(&mut self, data: &[u8]) -> Result<()> {
        self.commands
            .send(Command::Data(data.to_vec()))
            .map_err(|_| Error::Exited)
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.commands
            .send(Command::Resize { cols, rows })
            .map_err(|_| Error::Exited)
    }

    fn kill(&mut self) -> Result<()> {
        // Ignore the error: a closed channel means it is already gone.
        let _ = self.commands.send(Command::Close);
        Ok(())
    }
}

impl Drop for SshSession {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Close);
    }
}

/// A throwaway SSH server, so auth success *and* failure are proven against a
/// real protocol exchange rather than assumed. No system sshd, no fixtures, no
/// network beyond loopback.
#[cfg(test)]
mod fake_server {
    use std::net::SocketAddr;
    use std::sync::Arc;

    use russh::server::{Auth, ChannelOpenHandle, Handler, Msg, Server, Session};
    use russh::{Channel, ChannelId};

    pub const USER: &str = "tester";
    pub const PASSWORD: &str = "correct-horse";
    pub const BANNER: &str = "REMOTE-SHELL-READY";

    #[derive(Clone)]
    pub struct Fake;

    impl Server for Fake {
        type Handler = Fake;
        fn new_client(&mut self, _peer: Option<SocketAddr>) -> Fake {
            Fake
        }
    }

    impl Handler for Fake {
        type Error = russh::Error;

        // Accepts any key: this test is about our side offering one correctly.
        async fn auth_publickey(
            &mut self,
            _user: &str,
            _key: &russh::keys::ssh_key::PublicKey,
        ) -> Result<Auth, Self::Error> {
            Ok(Auth::Accept)
        }

        async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
            if user == USER && password == PASSWORD {
                Ok(Auth::Accept)
            } else {
                Ok(Auth::Reject {
                    proceed_with_methods: None,
                    partial_success: false,
                })
            }
        }

        async fn channel_open_session(
            &mut self,
            _channel: Channel<Msg>,
            reply: ChannelOpenHandle,
            _session: &mut Session,
        ) -> Result<(), Self::Error> {
            // Must be awaited: dropping the handle un-awaited sends
            // AdministrativelyProhibited (see its Drop impl).
            reply.accept().await;
            Ok(())
        }

        async fn shell_request(
            &mut self,
            channel: ChannelId,
            session: &mut Session,
        ) -> Result<(), Self::Error> {
            session.data(channel, BANNER.as_bytes().to_vec())?;
            Ok(())
        }

        // Echo, so the test can prove input reaches the server.
        async fn data(
            &mut self,
            channel: ChannelId,
            data: &[u8],
            session: &mut Session,
        ) -> Result<(), Self::Error> {
            session.data(channel, data.to_vec())?;
            Ok(())
        }
    }

    /// Start the server on an ephemeral port; returns the port it bound.
    pub fn spawn() -> u16 {
        let (port_tx, port_rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");

            rt.block_on(async move {
                let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
                    .await
                    .expect("bind");
                port_tx
                    .send(listener.local_addr().expect("addr").port())
                    .expect("send port");

                let config = Arc::new(russh::server::Config {
                    keys: vec![russh::keys::PrivateKey::random(
                        &mut rand::rng(),
                        russh::keys::Algorithm::Ed25519,
                    )
                    .expect("host key")],
                    auth_rejection_time: std::time::Duration::from_millis(0),
                    auth_rejection_time_initial: Some(std::time::Duration::from_millis(0)),
                    ..Default::default()
                });

                let mut server = Fake;
                let _ = server.run_on_socket(config, &listener).await;
            });
        });

        port_rx.recv().expect("server port")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Point the trust store at a scratch directory, once per test process.
    fn isolate_data_dir() {
        use std::sync::Once;
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let dir = std::env::temp_dir().join(format!("shaman-test-{}", std::process::id()));
            std::env::set_var("SHAMAN_DATA_DIR", &dir);
        });
    }

    #[test]
    fn label_reads_as_user_at_host() {
        let opts = SshOptions {
            host: "10.0.0.5".into(),
            port: 22,
            username: "root".into(),
            auth: SshAuth::Password {
                password: "hunter2".into(),
            },
            cols: 80,
            rows: 24,
            trust_new_key: false,
        };
        assert_eq!(opts.label(), "root@10.0.0.5");
    }

    #[test]
    fn an_unreachable_host_fails_as_unreachable_not_auth() {
        // Port 1 on localhost is reliably closed. The distinction matters: the
        // dialog tells the user to check the address rather than the password.
        let result = SshSession::connect(
            SessionId::next(),
            SshOptions {
                host: "127.0.0.1".into(),
                port: 1,
                username: "nobody".into(),
                auth: SshAuth::Password {
                    password: "nothing".into(),
                },
                cols: 80,
                rows: 24,
                trust_new_key: true,
            },
            |_| {},
            || {},
        );

        // SshSession has no Debug impl, so match rather than expect_err().
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("connecting to a closed port must fail"),
        };

        assert_eq!(err.kind, SshFailure::Unreachable, "got: {}", err.message);
    }

    #[test]
    fn a_host_that_swallows_the_connection_gives_up_on_our_schedule() {
        // The reported failure: a saved device that is off or on another
        // network. Such a host does not refuse the connection, it ignores it —
        // and Windows will retry the SYN for ~21s before reporting anything.
        // 192.0.2.0/24 is TEST-NET-1 (RFC 5737) and is never routable, so this
        // exercises the black-hole path rather than a fast refusal.
        //
        // The point of the assertion is the *bound*: the failure must arrive on
        // REACH_TIMEOUT's schedule, not the operating system's.
        let started = std::time::Instant::now();
        let result = SshSession::connect(
            SessionId::next(),
            SshOptions {
                host: "192.0.2.1".into(),
                port: 22,
                username: "nobody".into(),
                auth: SshAuth::Password {
                    password: "nothing".into(),
                },
                cols: 80,
                rows: 24,
                trust_new_key: true,
            },
            |_| {},
            || {},
        );
        let elapsed = started.elapsed();

        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("connecting to an unroutable address must fail"),
        };

        assert_eq!(err.kind, SshFailure::Unreachable, "got: {}", err.message);
        assert!(
            elapsed < REACH_TIMEOUT + Duration::from_secs(5),
            "gave up after {elapsed:?}, which is the OS timing out rather than us"
        );
    }

    /// The case the connect dialog is built around: a wrong password must come
    /// back as Auth, distinctly from a host that cannot be reached.
    #[test]
    fn a_rejected_password_reports_auth_failure() {
        use super::fake_server;
        isolate_data_dir();
        let port = fake_server::spawn();

        let result = SshSession::connect(
            SessionId::next(),
            SshOptions {
                host: "127.0.0.1".into(),
                port,
                username: fake_server::USER.into(),
                auth: SshAuth::Password {
                    password: "wrong-password".into(),
                },
                cols: 80,
                rows: 24,
                trust_new_key: true,
            },
            |_| {},
            || {},
        );

        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("a wrong password must not connect"),
        };
        assert_eq!(err.kind, SshFailure::Auth, "got: {}", err.message);
        assert!(
            err.message.contains("authentication failed"),
            "message should say what went wrong, got: {}",
            err.message
        );
    }

    /// Verification must *block*, and must do so before credentials are sent:
    /// a correct password to an unverified host is exactly the leak this
    /// feature exists to prevent.
    #[test]
    fn an_untrusted_host_key_is_refused_even_with_correct_credentials() {
        use super::fake_server;
        isolate_data_dir();
        let port = fake_server::spawn();

        let result = SshSession::connect(
            SessionId::next(),
            SshOptions {
                host: "127.0.0.1".into(),
                port,
                username: fake_server::USER.into(),
                auth: SshAuth::Password {
                    password: fake_server::PASSWORD.into(),
                },
                cols: 80,
                rows: 24,
                trust_new_key: false,
            },
            |_| {},
            || {},
        );

        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("an unverified host key must not connect"),
        };

        assert_eq!(err.kind, SshFailure::UnknownHostKey, "got: {}", err.message);
        assert!(
            err.fingerprint.is_some(),
            "the prompt needs the fingerprint to show"
        );
    }

    /// Key auth, proven against a real exchange: the server accepts any key
    /// here, so this exercises loading, offering and accepting a public key.
    #[test]
    fn a_private_key_authenticates() {
        use super::fake_server;
        isolate_data_dir();

        // Write a throwaway unencrypted key to a temp file.
        let key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .expect("generate key");
        let pem = key
            .to_openssh(russh::keys::ssh_key::LineEnding::LF)
            .expect("encode key");
        let path = std::env::temp_dir().join(format!("shaman-test-key-{}", std::process::id()));
        std::fs::write(&path, pem.as_bytes()).expect("write key");

        let port = fake_server::spawn();
        let result = SshSession::connect(
            SessionId::next(),
            SshOptions {
                host: "127.0.0.1".into(),
                port,
                username: fake_server::USER.into(),
                auth: SshAuth::Key {
                    path: path.to_string_lossy().into_owned(),
                    passphrase: String::new(),
                },
                cols: 80,
                rows: 24,
                trust_new_key: true,
            },
            |_| {},
            || {},
        );

        let _ = std::fs::remove_file(&path);

        match result {
            Ok(mut session) => {
                session.kill().ok();
            }
            Err(e) => panic!("key auth should succeed: {}", e.message),
        }
    }

    /// A missing or unreadable key must blame the key, not the server.
    #[test]
    fn a_missing_key_file_says_so() {
        use super::fake_server;
        isolate_data_dir();
        let port = fake_server::spawn();

        let result = SshSession::connect(
            SessionId::next(),
            SshOptions {
                host: "127.0.0.1".into(),
                port,
                username: fake_server::USER.into(),
                auth: SshAuth::Key {
                    path: r"C:
ope\id_ed25519"
                        .into(),
                    passphrase: String::new(),
                },
                cols: 80,
                rows: 24,
                trust_new_key: true,
            },
            |_| {},
            || {},
        );

        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("a missing key must not connect"),
        };
        assert_eq!(err.kind, SshFailure::Auth);
        assert!(
            err.message.contains("could not read the private key"),
            "should blame the key file, got: {}",
            err.message
        );
    }

    #[test]
    fn a_good_password_opens_a_working_shell() {
        use super::fake_server;
        use std::sync::{Arc, Mutex};

        isolate_data_dir();
        let port = fake_server::spawn();
        let output = Arc::new(Mutex::new(Vec::<u8>::new()));
        let sink = Arc::clone(&output);

        let result = SshSession::connect(
            SessionId::next(),
            SshOptions {
                host: "127.0.0.1".into(),
                port,
                username: fake_server::USER.into(),
                auth: SshAuth::Password {
                    password: fake_server::PASSWORD.into(),
                },
                cols: 80,
                rows: 24,
                trust_new_key: true,
            },
            move |chunk| sink.lock().unwrap().extend_from_slice(&chunk),
            || {},
        );

        let mut session = match result {
            Ok(s) => s,
            Err(e) => panic!("valid credentials should connect: {}", e.message),
        };

        // The banner proves a shell was started; the echo proves input flows.
        session.write(b"ping-from-client").expect("write");

        let start = std::time::Instant::now();
        loop {
            let seen = String::from_utf8_lossy(&output.lock().unwrap().clone()).to_string();
            if seen.contains(fake_server::BANNER) && seen.contains("ping-from-client") {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(15),
                "never saw banner + echo; got: {seen:?}"
            );
            std::thread::sleep(Duration::from_millis(25));
        }

        session.kill().ok();
    }
}
