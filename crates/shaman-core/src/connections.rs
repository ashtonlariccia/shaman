//! Saved SSH connections.
//!
//! Stored in `%APPDATA%\shaman\connections.json`. Two rules shape this file:
//!
//! * **Passwords are DPAPI blobs, never plaintext**, and they never leave the
//!   backend. The UI works with ids; [`Store::password`] is the only way to get
//!   a secret back, and only `ssh_connect_saved` calls it.
//! * **Entries are keyed by a stable id, not by host.** Editing a connection's
//!   address keeps its credentials attached — which matters here because the
//!   machines behind these addresses change.

use serde::{Deserialize, Serialize};

use crate::ssh::SshAuth;
use crate::vault::{data_dir, seal, to_hex, unseal};
use crate::{Error, Result};

const FILE: &str = "connections.json";

/// A saved connection as the UI sees it — no secret material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedConnection {
    pub id: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    /// "password" or "key" — shown in the manage dialog so it's obvious how a
    /// saved entry authenticates.
    #[serde(default = "password_kind")]
    pub auth_kind: String,
    /// A name you gave it, e.g. "prod-db". Display only; `host` is what we dial.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

fn password_kind() -> String {
    "password".to_string()
}

impl SavedConnection {
    /// `user@host` and, when it isn't the default, `:port`.
    pub fn label(&self) -> String {
        if self.port == 22 {
            format!("{}@{}", self.username, self.host)
        } else {
            format!("{}@{}:{}", self.username, self.host, self.port)
        }
    }

    /// Same host, port and user — used to decide whether to offer to save.
    pub fn matches(&self, host: &str, port: u16, username: &str) -> bool {
        self.host.eq_ignore_ascii_case(host) && self.port == port && self.username == username
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    #[serde(flatten)]
    connection: SavedConnection,
    /// DPAPI blob, hex encoded. The password, or the key's passphrase.
    secret: String,
    /// Private key path, when this entry authenticates with a key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Store {
    /// Whether to offer to save after connecting to an unknown host.
    /// "Never show again" clears this.
    #[serde(default = "default_true")]
    pub suggest_saving: bool,
    #[serde(default)]
    entries: Vec<Entry>,
}

fn default_true() -> bool {
    true
}

impl Default for Store {
    fn default() -> Self {
        Self {
            suggest_saving: true,
            entries: Vec::new(),
        }
    }
}

fn new_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("OS randomness unavailable");
    to_hex(&bytes)
}

impl Store {
    pub fn load() -> Result<Self> {
        let path = data_dir()?.join(FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| {
                Error::Other(anyhow::anyhow!(
                    "{} is corrupt ({e}); move it aside to start fresh",
                    path.display()
                ))
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::Io(e)),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = data_dir()?.join(FILE);
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| Error::Other(anyhow::anyhow!("could not serialise connections: {e}")))?;
        std::fs::write(&path, text)?;
        Ok(())
    }

    pub fn list(&self) -> Vec<SavedConnection> {
        self.entries.iter().map(|e| e.connection.clone()).collect()
    }

    pub fn find(&self, host: &str, port: u16, username: &str) -> Option<&SavedConnection> {
        self.entries
            .iter()
            .map(|e| &e.connection)
            .find(|c| c.matches(host, port, username))
    }

    /// Add a connection, or update the password of a matching one.
    ///
    /// Re-saving the same target must not create a duplicate row, and must keep
    /// the original id so anything referring to it still resolves.
    pub fn upsert(
        &mut self,
        host: &str,
        port: u16,
        username: &str,
        auth: &SshAuth,
    ) -> Result<SavedConnection> {
        let (kind, key_path, plaintext) = match auth {
            SshAuth::Password { password } => ("password", None, password.clone()),
            SshAuth::Key { path, passphrase } => ("key", Some(path.clone()), passphrase.clone()),
        };
        let secret = seal(&plaintext)?;

        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|e| e.connection.matches(host, port, username))
        {
            entry.secret = secret;
            entry.key_path = key_path;
            entry.connection.auth_kind = kind.to_string();
            // Deliberately leaves `name` alone: re-saving credentials must not
            // wipe a name you chose.
            return Ok(entry.connection.clone());
        }

        let connection = SavedConnection {
            id: new_id(),
            host: host.to_string(),
            port,
            username: username.to_string(),
            auth_kind: kind.to_string(),
            name: None,
        };
        self.entries.push(Entry {
            connection: connection.clone(),
            secret,
            key_path,
        });
        Ok(connection)
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.connection.id != id);
        self.entries.len() != before
    }

    /// Rebuild the auth for a saved connection, decrypting its secret.
    ///
    /// Backend-only: neither the password nor the passphrase may be returned to
    /// the frontend.
    pub fn auth(&self, id: &str) -> Result<SshAuth> {
        let entry = self
            .entries
            .iter()
            .find(|e| e.connection.id == id)
            .ok_or_else(|| Error::Other(anyhow::anyhow!("no saved connection {id}")))?;

        let secret = unseal(&entry.secret)?;

        Ok(
            match (entry.connection.auth_kind.as_str(), &entry.key_path) {
                ("key", Some(path)) => SshAuth::Key {
                    path: path.clone(),
                    passphrase: secret,
                },
                _ => SshAuth::Password { password: secret },
            },
        )
    }

    /// Set or clear the display name. An empty name clears it.
    pub fn rename(&mut self, id: &str, name: &str) -> Result<SavedConnection> {
        let entry = self
            .entries
            .iter_mut()
            .find(|e| e.connection.id == id)
            .ok_or_else(|| Error::Other(anyhow::anyhow!("no saved connection {id}")))?;

        let trimmed = name.trim();
        entry.connection.name = (!trimmed.is_empty()).then(|| trimmed.to_string());
        Ok(entry.connection.clone())
    }

    pub fn get(&self, id: &str) -> Option<&SavedConnection> {
        self.entries
            .iter()
            .map(|e| &e.connection)
            .find(|c| c.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn isolate() {
        use std::sync::Once;
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let dir = std::env::temp_dir().join(format!("shaman-conn-{}", std::process::id()));
            std::env::set_var("SHAMAN_DATA_DIR", &dir);
        });
    }

    #[test]
    fn labels_hide_the_default_port_but_show_others() {
        let c = SavedConnection {
            id: "x".into(),
            host: "10.0.0.5".into(),
            port: 22,
            username: "root".into(),
            auth_kind: "password".into(),
            name: None,
        };
        assert_eq!(c.label(), "root@10.0.0.5");

        let c = SavedConnection { port: 2222, ..c };
        assert_eq!(c.label(), "root@10.0.0.5:2222");
    }

    #[test]
    #[cfg(windows)]
    fn a_saved_password_round_trips_but_is_not_stored_in_the_clear() {
        isolate();
        let mut store = Store::default();
        let saved = store
            .upsert(
                "10.0.0.5",
                22,
                "root",
                &SshAuth::Password {
                    password: "hunter2".into(),
                },
            )
            .unwrap();

        let json = serde_json::to_string(&store).unwrap();
        assert!(!json.contains("hunter2"), "password must not be plaintext");

        match store.auth(&saved.id).unwrap() {
            SshAuth::Password { password } => assert_eq!(password, "hunter2"),
            other => panic!("expected a password, got {other:?}"),
        }
    }

    #[test]
    #[cfg(windows)]
    fn re_saving_the_same_target_updates_rather_than_duplicates() {
        isolate();
        let mut store = Store::default();
        let first = store
            .upsert(
                "host",
                22,
                "me",
                &SshAuth::Password {
                    password: "old".into(),
                },
            )
            .unwrap();
        let second = store
            .upsert(
                "host",
                22,
                "me",
                &SshAuth::Password {
                    password: "new".into(),
                },
            )
            .unwrap();

        assert_eq!(store.list().len(), 1, "must not duplicate");
        assert_eq!(first.id, second.id, "id must be stable across updates");
        match store.auth(&first.id).unwrap() {
            SshAuth::Password { password } => assert_eq!(password, "new"),
            other => panic!("expected a password, got {other:?}"),
        }
    }

    #[test]
    #[cfg(windows)]
    fn the_same_host_with_a_different_user_is_a_separate_entry() {
        isolate();
        let mut store = Store::default();
        store
            .upsert(
                "host",
                22,
                "alice",
                &SshAuth::Password {
                    password: "a".into(),
                },
            )
            .unwrap();
        store
            .upsert(
                "host",
                22,
                "bob",
                &SshAuth::Password {
                    password: "b".into(),
                },
            )
            .unwrap();
        assert_eq!(store.list().len(), 2);
    }

    #[test]
    #[cfg(windows)]
    fn removing_works_and_is_idempotent() {
        isolate();
        let mut store = Store::default();
        let saved = store
            .upsert(
                "host",
                22,
                "me",
                &SshAuth::Password {
                    password: "pw".into(),
                },
            )
            .unwrap();

        assert!(store.remove(&saved.id));
        assert!(store.list().is_empty());
        assert!(
            !store.remove(&saved.id),
            "second remove reports nothing done"
        );
    }

    #[test]
    #[cfg(windows)]
    fn a_key_connection_round_trips_with_its_path_and_passphrase() {
        isolate();
        let mut store = Store::default();
        let saved = store
            .upsert(
                "host",
                22,
                "me",
                &SshAuth::Key {
                    path: r"C:\Users\me\.ssh\id_ed25519".into(),
                    passphrase: "secret-phrase".into(),
                },
            )
            .unwrap();

        assert_eq!(saved.auth_kind, "key");
        let json = serde_json::to_string(&store).unwrap();
        assert!(
            !json.contains("secret-phrase"),
            "passphrase must not be plaintext"
        );

        match store.auth(&saved.id).unwrap() {
            SshAuth::Key { path, passphrase } => {
                assert!(path.ends_with("id_ed25519"));
                assert_eq!(passphrase, "secret-phrase");
            }
            other => panic!("expected a key, got {other:?}"),
        }
    }

    #[test]
    #[cfg(windows)]
    fn renaming_sets_and_clears_the_name() {
        isolate();
        let mut store = Store::default();
        let saved = store
            .upsert(
                "10.0.0.5",
                22,
                "me",
                &SshAuth::Password {
                    password: "pw".into(),
                },
            )
            .unwrap();
        assert_eq!(saved.name, None, "new entries start unnamed");

        assert_eq!(
            store
                .rename(&saved.id, "  prod-db  ")
                .unwrap()
                .name
                .as_deref(),
            Some("prod-db"),
            "names are trimmed"
        );

        assert_eq!(
            store.rename(&saved.id, "   ").unwrap().name,
            None,
            "an empty name clears it rather than storing blanks"
        );
    }

    #[test]
    #[cfg(windows)]
    fn re_saving_credentials_keeps_the_name() {
        isolate();
        let mut store = Store::default();
        let saved = store
            .upsert(
                "10.0.0.5",
                22,
                "me",
                &SshAuth::Password {
                    password: "old".into(),
                },
            )
            .unwrap();
        store.rename(&saved.id, "prod-db").unwrap();

        let again = store
            .upsert(
                "10.0.0.5",
                22,
                "me",
                &SshAuth::Password {
                    password: "new".into(),
                },
            )
            .unwrap();
        assert_eq!(again.name.as_deref(), Some("prod-db"));
    }

    #[test]
    fn find_matches_case_insensitively_on_host_only() {
        let mut store = Store::default();
        store.entries.push(Entry {
            connection: SavedConnection {
                id: "1".into(),
                host: "Server.Local".into(),
                port: 22,
                username: "me".into(),
                auth_kind: "password".into(),
                name: None,
            },
            secret: String::new(),
            key_path: None,
        });

        assert!(store.find("server.local", 22, "me").is_some());
        assert!(
            store.find("server.local", 22, "ME").is_none(),
            "user is case-sensitive"
        );
        assert!(
            store.find("server.local", 2222, "me").is_none(),
            "port matters"
        );
    }

    #[test]
    fn suggest_saving_defaults_on_for_an_empty_file() {
        let store: Store = serde_json::from_str("{}").unwrap();
        assert!(store.suggest_saving);
    }
}
