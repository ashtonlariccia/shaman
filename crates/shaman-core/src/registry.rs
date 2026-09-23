//! The set of live sessions.
//!
//! Stores `Box<dyn Session>` rather than a concrete type, so an SSH connection
//! and a local shell sit side by side without the registry (or anything above
//! it) knowing the difference.

use std::collections::HashMap;

use serde::Serialize;

use crate::session::{Session, SessionId, SessionKind};

/// What the UI needs to render one row in the sidebar.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: SessionId,
    pub kind: SessionKind,
    pub label: String,
    pub elevated: bool,
}

#[derive(Default)]
pub struct Registry {
    sessions: HashMap<SessionId, Box<dyn Session>>,
    /// Sidebar order. A HashMap alone would shuffle tabs on every render.
    order: Vec<SessionId>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, session: Box<dyn Session>) -> SessionId {
        let id = session.id();
        self.sessions.insert(id, session);
        self.order.push(id);
        id
    }

    pub fn get_mut(&mut self, id: SessionId) -> Option<&mut (dyn Session + 'static)> {
        self.sessions.get_mut(&id).map(|s| s.as_mut())
    }

    /// Remove a session, handing back ownership so the caller decides when it
    /// dies (dropping it tears down the PTY and its job object).
    pub fn remove(&mut self, id: SessionId) -> Option<Box<dyn Session>> {
        self.order.retain(|&x| x != id);
        self.sessions.remove(&id)
    }

    /// Sidebar contents, in the order sessions were opened.
    pub fn list(&self) -> Vec<SessionSummary> {
        self.order
            .iter()
            .filter_map(|id| self.sessions.get(id))
            .map(|s| SessionSummary {
                id: s.id(),
                kind: s.kind().clone(),
                label: s.kind().label(),
                elevated: s.elevated(),
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Result;

    /// Minimal stand-in so registry behaviour can be tested without spawning
    /// real shells.
    #[derive(Debug)]
    struct Fake {
        id: SessionId,
        kind: SessionKind,
    }

    impl Session for Fake {
        fn id(&self) -> SessionId {
            self.id
        }
        fn kind(&self) -> &SessionKind {
            &self.kind
        }
        fn elevated(&self) -> bool {
            false
        }
        fn write(&mut self, _: &[u8]) -> Result<()> {
            Ok(())
        }
        fn resize(&mut self, _: u16, _: u16) -> Result<()> {
            Ok(())
        }
        fn kill(&mut self) -> Result<()> {
            Ok(())
        }
    }

    fn fake(kind: SessionKind) -> Box<dyn Session> {
        Box::new(Fake {
            id: SessionId::next(),
            kind,
        })
    }

    #[test]
    fn preserves_insertion_order_so_tabs_do_not_shuffle() {
        let mut reg = Registry::new();
        let a = reg.insert(fake(SessionKind::Cmd));
        let b = reg.insert(fake(SessionKind::PowerShell));
        let c = reg.insert(fake(SessionKind::Wsl {
            distro: "main".into(),
        }));

        let ids: Vec<_> = reg.list().iter().map(|s| s.id).collect();
        assert_eq!(ids, vec![a, b, c]);
    }

    #[test]
    fn removing_a_session_drops_it_from_the_listing() {
        let mut reg = Registry::new();
        let a = reg.insert(fake(SessionKind::Cmd));
        let b = reg.insert(fake(SessionKind::Cmd));

        assert!(reg.remove(a).is_some());
        assert_eq!(reg.len(), 1);

        let ids: Vec<_> = reg.list().iter().map(|s| s.id).collect();
        assert_eq!(ids, vec![b], "removed session must leave the order too");

        assert!(reg.remove(a).is_none(), "double remove should be harmless");
    }

    #[test]
    fn labels_come_from_the_session_kind() {
        let mut reg = Registry::new();
        reg.insert(fake(SessionKind::Wsl {
            distro: "main".into(),
        }));
        assert_eq!(reg.list()[0].label, "WSL · main");
    }
}
