//! Trust-on-first-use host key store.
//!
//! Without this, Shaman accepts whatever key a server presents. That is merely
//! sloppy while passwords are typed by hand — you are present, and you just
//! named the host. It becomes dangerous the moment credentials are *saved and
//! autofilled*, because the app would then hand a stored password to whatever
//! happens to answer on that address. These hosts change IP, so that is not a
//! hypothetical.
//!
//! Three outcomes:
//!
//! * **Unknown** — first sight of this host. Ask, then remember.
//! * **Match** — connect silently.
//! * **Changed** — refuse, loudly. Either the server was rebuilt, or something
//!   is impersonating it. Only an explicit human decision can overwrite it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::vault::data_dir;
use crate::{Error, Result};

const FILE: &str = "known_hosts.json";

/// One trusted entry, split back into its parts for display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedHost {
    /// The raw store key, used to remove the entry unambiguously.
    pub key: String,
    pub host: String,
    pub port: u16,
    pub fingerprint: String,
}

/// What we knew about a host before this connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum Verdict {
    Unknown,
    Match,
    Changed { expected: String },
}

/// `host:port` -> fingerprint (`SHA256:...`).
///
/// A BTreeMap so the file has a stable order and diffs sensibly.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct KnownHosts {
    #[serde(default)]
    hosts: BTreeMap<String, String>,
}

pub fn key_for(host: &str, port: u16) -> String {
    // Match OpenSSH's convention of only annotating non-default ports.
    if port == 22 {
        host.to_ascii_lowercase()
    } else {
        format!("[{}]:{}", host.to_ascii_lowercase(), port)
    }
}

/// Inverse of [`key_for`]: `[host]:port` splits, anything else is port 22.
fn split_key(key: &str) -> (String, u16) {
    if let Some(rest) = key.strip_prefix('[') {
        if let Some((host, port)) = rest.rsplit_once("]:") {
            if let Ok(port) = port.parse() {
                return (host.to_string(), port);
            }
        }
    }
    (key.to_string(), 22)
}

impl KnownHosts {
    pub fn load() -> Result<Self> {
        let path = data_dir()?.join(FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| {
                Error::Other(anyhow::anyhow!(
                    "{} is corrupt ({e}); move it aside to start fresh",
                    path.display()
                ))
            }),
            // No file yet simply means nothing has been trusted.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::Io(e)),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = data_dir()?.join(FILE);
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| Error::Other(anyhow::anyhow!("could not serialise known hosts: {e}")))?;
        std::fs::write(&path, text)?;
        Ok(())
    }

    pub fn verdict(&self, host: &str, port: u16, fingerprint: &str) -> Verdict {
        match self.hosts.get(&key_for(host, port)) {
            None => Verdict::Unknown,
            Some(known) if known == fingerprint => Verdict::Match,
            Some(known) => Verdict::Changed {
                expected: known.clone(),
            },
        }
    }

    /// Trust this fingerprint from now on, replacing any previous one.
    pub fn trust(&mut self, host: &str, port: u16, fingerprint: &str) {
        self.hosts
            .insert(key_for(host, port), fingerprint.to_string());
    }

    pub fn forget(&mut self, host: &str, port: u16) {
        self.hosts.remove(&key_for(host, port));
    }

    /// Every trusted entry, for the Trusted Host Keys dialog.
    pub fn list(&self) -> Vec<TrustedHost> {
        self.hosts
            .iter()
            .map(|(key, fingerprint)| {
                let (host, port) = split_key(key);
                TrustedHost {
                    key: key.clone(),
                    host,
                    port,
                    fingerprint: fingerprint.clone(),
                }
            })
            .collect()
    }

    /// Remove by raw store key. Returns whether anything was removed.
    pub fn forget_key(&mut self, key: &str) -> bool {
        self.hosts.remove(key).is_some()
    }

    pub fn len(&self) -> usize {
        self.hosts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hosts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FP_A: &str = "SHA256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const FP_B: &str = "SHA256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn default_port_is_not_annotated_but_others_are() {
        assert_eq!(key_for("Example.COM", 22), "example.com");
        assert_eq!(key_for("example.com", 2222), "[example.com]:2222");
    }

    #[test]
    fn the_same_host_on_a_different_port_is_a_different_entry() {
        let mut known = KnownHosts::default();
        known.trust("10.0.0.5", 22, FP_A);

        assert_eq!(known.verdict("10.0.0.5", 22, FP_A), Verdict::Match);
        assert_eq!(known.verdict("10.0.0.5", 2222, FP_A), Verdict::Unknown);
    }

    #[test]
    fn an_unseen_host_is_unknown_and_a_seen_one_matches() {
        let mut known = KnownHosts::default();
        assert_eq!(known.verdict("host", 22, FP_A), Verdict::Unknown);

        known.trust("host", 22, FP_A);
        assert_eq!(known.verdict("host", 22, FP_A), Verdict::Match);
    }

    #[test]
    fn a_different_key_reports_changed_and_carries_the_expected_one() {
        // The case that matters: this must never silently pass.
        let mut known = KnownHosts::default();
        known.trust("host", 22, FP_A);

        match known.verdict("host", 22, FP_B) {
            Verdict::Changed { expected } => assert_eq!(expected, FP_A),
            other => panic!("a changed key must not be accepted, got {other:?}"),
        }
    }

    #[test]
    fn trusting_again_replaces_the_old_fingerprint() {
        let mut known = KnownHosts::default();
        known.trust("host", 22, FP_A);
        known.trust("host", 22, FP_B);

        assert_eq!(known.verdict("host", 22, FP_B), Verdict::Match);
        assert_eq!(known.len(), 1, "replaced, not duplicated");
    }

    #[test]
    fn listing_splits_the_key_back_into_host_and_port() {
        let mut known = KnownHosts::default();
        known.trust("example.com", 22, FP_A);
        known.trust("10.0.0.5", 2222, FP_B);

        let mut listed = known.list();
        listed.sort_by(|a, b| a.host.cmp(&b.host));

        assert_eq!((listed[0].host.as_str(), listed[0].port), ("10.0.0.5", 2222));
        assert_eq!((listed[1].host.as_str(), listed[1].port), ("example.com", 22));
    }

    #[test]
    fn forgetting_by_key_removes_exactly_one_entry() {
        let mut known = KnownHosts::default();
        known.trust("a.example", 22, FP_A);
        known.trust("b.example", 2222, FP_B);

        let key = known.list().iter().find(|t| t.host == "b.example").unwrap().key.clone();
        assert!(known.forget_key(&key));
        assert_eq!(known.len(), 1);
        assert!(!known.forget_key(&key), "second removal reports nothing done");
    }

    #[test]
    fn hostnames_are_matched_case_insensitively() {
        let mut known = KnownHosts::default();
        known.trust("Server.Local", 22, FP_A);
        assert_eq!(known.verdict("server.local", 22, FP_A), Verdict::Match);
    }
}
