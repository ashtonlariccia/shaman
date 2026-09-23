//! Core session engine for Shaman.
//!
//! Everything the UI touches is a [`Session`]: a bidirectional byte stream with
//! a size and a lifecycle. Local shells, elevated shells, WSL distros, and SSH
//! connections are all the same shape to the layers above, so adding a new kind
//! of terminal means implementing one trait rather than touching the frontend.

use std::fmt;

pub mod connections;
pub mod elevate;
pub mod helper;
pub mod known_hosts;
pub mod pins;
pub mod profiles;
pub mod proto;
pub mod pty;
pub mod pump;
pub mod registry;
pub mod session;
pub mod vault;
pub mod ssh;
pub mod win;

pub use elevate::{is_elevated, spawn_deelevated};
pub use helper::{Helper, HelperSession};
pub use profiles::ShellProfile;
pub use pty::{PtyOptions, PtySession};
pub use pump::CoalesceConfig;
pub use registry::{Registry, SessionSummary};
pub use session::{Session, SessionId, SessionKind};
pub use connections::{SavedConnection, Store as ConnectionStore};
pub use known_hosts::{KnownHosts, TrustedHost, Verdict};
pub use pins::{Pin, PinKind, Store as PinStore};
pub use ssh::{DiscoveredKey, SshAuth, SshError, SshFailure, SshOptions, SshSession};
pub use win::harden_dll_search;

/// Errors surfaced by the core engine.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("session {0} not found")]
    NotFound(SessionId),

    #[error("session has already exited")]
    Exited,

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Build/version banner, surfaced in the UI's about screen.
pub struct Version;

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "shaman-core {}", env!("CARGO_PKG_VERSION"))
    }
}
