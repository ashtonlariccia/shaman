//! Pinned connections — the quick-open strip along the bottom of the window.
//!
//! Stored in `%APPDATA%\shaman\pins.json`. Two rules shape this file:
//!
//! * **A pin stores what to open, never how it looks.** It is a shell profile
//!   id or a saved-connection id, and the name shown in the strip is resolved
//!   from the profile list or the connection store every time it is drawn — so
//!   renaming a saved connection renames its pin. [`Pin::label`] is only a
//!   fallback for a target that has since gone away.
//! * **One row per target.** Pinning something already pinned refreshes its
//!   stored label instead of adding a second, identical button.

use serde::{Deserialize, Serialize};

use crate::vault::data_dir;
use crate::{Error, Result};

const FILE: &str = "pins.json";

/// What kind of thing a pin points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PinKind {
    /// A detected shell, identified by [`crate::ShellProfile::id`].
    Local,
    /// A saved SSH connection, identified by its store id.
    Saved,
}

/// One button on the quick-open strip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pin {
    pub kind: PinKind,
    /// Profile id, or saved-connection id — whichever [`Pin::kind`] says.
    pub target: String,
    /// The name at the time of pinning. Display fallback only; the live name
    /// wins whenever the target still resolves.
    #[serde(default)]
    pub label: String,
}

impl Pin {
    /// Pins are the same button if they open the same thing. The label is
    /// deliberately not part of this — a renamed target is not a new pin.
    fn points_at(&self, kind: PinKind, target: &str) -> bool {
        self.kind == kind && self.target == target
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pins: Vec<Pin>,
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
            .map_err(|e| Error::Other(anyhow::anyhow!("could not serialise pins: {e}")))?;
        std::fs::write(&path, text)?;
        Ok(())
    }

    /// Every pin, in the order they were added — which is the order they appear
    /// in the strip, so it must stay stable.
    pub fn list(&self) -> Vec<Pin> {
        self.pins.clone()
    }

    /// Add a pin, or refresh the stored label of the one already holding that
    /// target. Returns whether the strip actually gained a button.
    pub fn add(&mut self, pin: Pin) -> bool {
        if let Some(existing) = self
            .pins
            .iter_mut()
            .find(|p| p.points_at(pin.kind, &pin.target))
        {
            existing.label = pin.label;
            return false;
        }

        self.pins.push(pin);
        true
    }

    /// Move a pin to `index`, counted in the list *as it will be afterwards*.
    ///
    /// Applied to whatever is on disk right now rather than by overwriting the
    /// order wholesale, so a second window that pinned something in the
    /// meantime does not lose it to a stale list. Returns false if that pin
    /// isn't there.
    pub fn move_to(&mut self, kind: PinKind, target: &str, index: usize) -> bool {
        let Some(from) = self.pins.iter().position(|p| p.points_at(kind, target)) else {
            return false;
        };

        let pin = self.pins.remove(from);
        // Clamped against the shortened list: dropping past the right-hand end
        // is a normal gesture, not an error.
        self.pins.insert(index.min(self.pins.len()), pin);
        true
    }

    pub fn remove(&mut self, kind: PinKind, target: &str) -> bool {
        let before = self.pins.len();
        self.pins.retain(|p| !p.points_at(kind, target));
        self.pins.len() != before
    }

    pub fn contains(&self, kind: PinKind, target: &str) -> bool {
        self.pins.iter().any(|p| p.points_at(kind, target))
    }

    /// Drop the pin for a saved connection that has been deleted — otherwise the
    /// strip keeps a button that can no longer open anything.
    pub fn forget_connection(&mut self, id: &str) -> bool {
        self.remove(PinKind::Saved, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(kind: PinKind, target: &str, label: &str) -> Pin {
        Pin {
            kind,
            target: target.into(),
            label: label.into(),
        }
    }

    #[test]
    fn pinning_the_same_target_twice_does_not_duplicate() {
        let mut store = Store::default();
        assert!(store.add(pin(PinKind::Local, "cmd", "Command Prompt")));
        assert!(!store.add(pin(PinKind::Local, "cmd", "Command Prompt")));
        assert_eq!(store.list().len(), 1);
    }

    #[test]
    fn re_pinning_refreshes_the_stored_label_in_place() {
        let mut store = Store::default();
        store.add(pin(PinKind::Saved, "abc", "10.0.0.5"));
        store.add(pin(PinKind::Saved, "abc", "prod-db"));

        let pins = store.list();
        assert_eq!(pins.len(), 1, "a rename is not a new pin");
        assert_eq!(pins[0].label, "prod-db");
    }

    #[test]
    fn kind_is_part_of_the_identity() {
        // A shell profile and a saved connection could share an id string; they
        // are still two different buttons.
        let mut store = Store::default();
        assert!(store.add(pin(PinKind::Local, "x", "shell")));
        assert!(store.add(pin(PinKind::Saved, "x", "host")));
        assert_eq!(store.list().len(), 2);
    }

    #[test]
    fn order_is_the_order_they_were_pinned() {
        let mut store = Store::default();
        store.add(pin(PinKind::Local, "cmd", "Command Prompt"));
        store.add(pin(PinKind::Saved, "abc", "prod-db"));
        store.add(pin(PinKind::Local, "pwsh", "PowerShell 7"));

        let targets: Vec<_> = store.list().into_iter().map(|p| p.target).collect();
        assert_eq!(targets, ["cmd", "abc", "pwsh"]);
    }

    /// The strip's order after `targets` have been pinned in that sequence.
    fn strip(targets: &[&str]) -> Store {
        let mut store = Store::default();
        for t in targets {
            store.add(pin(PinKind::Local, t, t));
        }
        store
    }

    fn order(store: &Store) -> Vec<String> {
        store.list().into_iter().map(|p| p.target).collect()
    }

    #[test]
    fn a_pin_can_be_dragged_left_and_right() {
        let mut store = strip(&["a", "b", "c", "d"]);

        assert!(store.move_to(PinKind::Local, "d", 0));
        assert_eq!(order(&store), ["d", "a", "b", "c"]);

        assert!(store.move_to(PinKind::Local, "d", 2));
        assert_eq!(order(&store), ["a", "b", "d", "c"]);
    }

    #[test]
    fn dropping_past_the_end_lands_at_the_end() {
        let mut store = strip(&["a", "b", "c"]);

        // The gesture is "drop it to the right of everything", which should not
        // need the caller to know how long the list is.
        assert!(store.move_to(PinKind::Local, "a", 99));
        assert_eq!(order(&store), ["b", "c", "a"]);
    }

    #[test]
    fn moving_a_pin_onto_itself_changes_nothing() {
        let mut store = strip(&["a", "b", "c"]);

        assert!(store.move_to(PinKind::Local, "b", 1));
        assert_eq!(order(&store), ["a", "b", "c"]);
    }

    #[test]
    fn moving_never_duplicates_or_drops_a_pin() {
        // The failure that would matter: an index off by one that leaves the
        // dragged pin in twice, or not at all.
        let mut store = strip(&["a", "b", "c", "d", "e"]);
        for index in 0..5 {
            assert!(store.move_to(PinKind::Local, "c", index));
            let mut seen = order(&store);
            seen.sort();
            assert_eq!(seen, ["a", "b", "c", "d", "e"], "lost or duplicated at {index}");
        }
    }

    #[test]
    fn moving_something_that_is_not_pinned_reports_nothing_done() {
        let mut store = strip(&["a"]);
        assert!(!store.move_to(PinKind::Local, "nope", 0));
        assert!(!store.move_to(PinKind::Saved, "a", 0), "kind is part of the match");
        assert_eq!(order(&store), ["a"]);
    }

    #[test]
    fn removing_works_and_is_idempotent() {
        let mut store = Store::default();
        store.add(pin(PinKind::Local, "cmd", "Command Prompt"));

        assert!(store.remove(PinKind::Local, "cmd"));
        assert!(!store.remove(PinKind::Local, "cmd"));
        assert!(store.list().is_empty());
    }

    #[test]
    fn deleting_a_saved_connection_takes_its_pin_with_it() {
        let mut store = Store::default();
        store.add(pin(PinKind::Saved, "abc", "prod-db"));
        store.add(pin(PinKind::Local, "abc", "Command Prompt"));

        assert!(store.forget_connection("abc"));
        assert!(
            store.contains(PinKind::Local, "abc"),
            "a local pin sharing the id string must survive"
        );
        assert!(!store.forget_connection("abc"));
    }

    #[test]
    fn an_empty_file_is_an_empty_strip() {
        let store: Store = serde_json::from_str("{}").unwrap();
        assert!(store.list().is_empty());
    }

    #[test]
    fn pins_round_trip_through_json() {
        let mut store = Store::default();
        store.add(pin(PinKind::Saved, "abc", "prod-db"));

        let json = serde_json::to_string(&store).unwrap();
        assert!(json.contains(r#""kind":"saved""#), "got {json}");

        let back: Store = serde_json::from_str(&json).unwrap();
        assert_eq!(back.list(), store.list());
    }
}
