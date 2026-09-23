//! The session abstraction.
//!
//! A session is deliberately minimal: bytes in, bytes out, a size, and a way to
//! die. Anything richer (shell detection, credentials, elevation) is handled by
//! whoever *constructs* the session, not by this interface.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// Opaque, stable identifier for a live session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub u64);

impl SessionId {
    /// Allocate a process-unique id.
    pub fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        SessionId(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// What kind of terminal a session is backed by.
///
/// This is metadata for the sidebar — it deliberately does not change how the
/// session is *driven*, only how it is presented and created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionKind {
    Cmd,
    PowerShell,
    Pwsh,
    Wsl { distro: String },
    Ssh { profile: String },
    Custom { label: String },
}

impl SessionKind {
    /// Short label for the sidebar.
    pub fn label(&self) -> String {
        match self {
            SessionKind::Cmd => "Command Prompt".into(),
            SessionKind::PowerShell => "PowerShell".into(),
            SessionKind::Pwsh => "PowerShell 7".into(),
            SessionKind::Wsl { distro } => format!("WSL · {distro}"),
            SessionKind::Ssh { profile } => format!("SSH · {profile}"),
            SessionKind::Custom { label } => label.clone(),
        }
    }
}

/// A live terminal session.
///
/// Output is *pushed* to a subscriber rather than polled, so the transport can
/// coalesce writes before they cross the IPC boundary.
pub trait Session: Send {
    fn id(&self) -> SessionId;

    fn kind(&self) -> &SessionKind;

    /// Whether this session runs at high integrity.
    fn elevated(&self) -> bool;

    /// Send user input toward the terminal.
    fn write(&mut self, data: &[u8]) -> crate::Result<()>;

    /// Inform the terminal of a new viewport size.
    fn resize(&mut self, cols: u16, rows: u16) -> crate::Result<()>;

    /// Terminate the session and everything it spawned.
    fn kill(&mut self) -> crate::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_are_unique_and_monotonic() {
        let a = SessionId::next();
        let b = SessionId::next();
        assert!(b.0 > a.0, "expected monotonic ids, got {a} then {b}");
    }

    #[test]
    fn wsl_sessions_label_with_their_distro() {
        let kind = SessionKind::Wsl {
            distro: "main".into(),
        };
        assert_eq!(kind.label(), "WSL · main");
    }
}
