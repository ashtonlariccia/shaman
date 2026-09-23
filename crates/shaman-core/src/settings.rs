//! Appearance settings, stored in `%APPDATA%\shaman\settings.json`.
//!
//! Global rather than per-terminal, deliberately: one look for every tab is
//! what makes a window of mixed shells read as one application. Per-profile
//! overrides can come later without changing this file's shape — they would be
//! a second layer on top, not a replacement.
//!
//! Every field has a default that reproduces the look Shaman shipped with, so
//! an absent or partial `settings.json` is not an error and a new field added
//! here does not invalidate an existing file.

use serde::{Deserialize, Serialize};

use crate::vault::{data_dir, strip_bom};
use crate::{Error, Result};

const FILE: &str = "settings.json";

/// How the cursor is drawn. Mirrors xterm's `cursorStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum CursorShape {
    #[default]
    Block,
    Bar,
    Underline,
}

/// The material drawn behind a translucent window.
///
/// `None` is a plain translucent window with nothing behind it; `Acrylic` is
/// the frosted blur Windows Terminal uses. Mica is deliberately absent: it
/// tints from the desktop wallpaper and ignores opacity, which makes it fight
/// the opacity slider rather than compose with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Material {
    #[default]
    None,
    Acrylic,
}

fn default_font_family() -> String {
    "Cascadia Mono".into()
}
fn default_font_size() -> u16 {
    13
}
fn default_cursor_color() -> String {
    "#f5e0dc".into()
}
fn default_cursor_blink() -> bool {
    true
}
fn default_opacity() -> u8 {
    100
}

/// What every terminal looks like.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Appearance {
    #[serde(default = "default_font_family")]
    pub font_family: String,

    #[serde(default = "default_font_size")]
    pub font_size: u16,

    #[serde(default)]
    pub cursor_shape: CursorShape,

    #[serde(default = "default_cursor_blink")]
    pub cursor_blink: bool,

    #[serde(default = "default_cursor_color")]
    pub cursor_color: String,

    /// Percent. 100 is fully opaque; the window only goes translucent below it.
    #[serde(default = "default_opacity")]
    pub background_opacity: u8,

    #[serde(default)]
    pub material: Material,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            font_family: default_font_family(),
            font_size: default_font_size(),
            cursor_shape: CursorShape::default(),
            cursor_blink: default_cursor_blink(),
            cursor_color: default_cursor_color(),
            background_opacity: default_opacity(),
            material: Material::default(),
        }
    }
}

impl Appearance {
    /// Clamp anything a hand-edited file (or a future UI) could get wrong.
    ///
    /// Applied on load and on save, so a bad value can neither be stored nor
    /// reach xterm. A zero font size renders nothing at all, and a fully
    /// transparent background makes the window unclickable-looking — both are
    /// states a user could not get out of from inside the app.
    pub fn sanitised(mut self) -> Self {
        self.font_size = self.font_size.clamp(6, 72);
        self.background_opacity = self.background_opacity.clamp(20, 100);

        if self.font_family.trim().is_empty() {
            self.font_family = default_font_family();
        }
        if !is_hex_colour(&self.cursor_color) {
            self.cursor_color = default_cursor_color();
        }
        self
    }

    /// Whether the window needs to be translucent for this to render.
    pub fn wants_transparency(&self) -> bool {
        self.background_opacity < 100 || self.material != Material::None
    }
}

/// `#rgb` or `#rrggbb`. Deliberately strict: the value goes straight into
/// xterm's theme, which silently ignores anything it cannot parse.
fn is_hex_colour(s: &str) -> bool {
    let Some(hex) = s.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 3 | 6) && hex.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn load() -> Result<Appearance> {
    let path = data_dir()?.join(FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let parsed: Appearance = serde_json::from_str(strip_bom(&text)).map_err(|e| {
                Error::Other(anyhow::anyhow!(
                    "{} is corrupt ({e}); move it aside to start fresh",
                    path.display()
                ))
            })?;
            Ok(parsed.sanitised())
        }
        // No file yet is the normal first run, not a failure.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Appearance::default()),
        Err(e) => Err(Error::Io(e)),
    }
}

pub fn save(appearance: &Appearance) -> Result<Appearance> {
    let sane = appearance.clone().sanitised();
    let path = data_dir()?.join(FILE);
    let text = serde_json::to_string_pretty(&sane)
        .map_err(|e| Error::Other(anyhow::anyhow!("could not serialise settings: {e}")))?;
    std::fs::write(&path, text)?;
    Ok(sane)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_object_yields_the_shipped_defaults() {
        let parsed: Appearance = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, Appearance::default());
    }

    /// Notepad and PowerShell both write UTF-8 with a BOM, and this file is one
    /// a user might reasonably hand-edit.
    #[test]
    fn a_byte_order_mark_does_not_make_the_file_corrupt() {
        let with_bom = "\u{feff}{\"fontSize\": 20}";
        assert!(
            serde_json::from_str::<Appearance>(with_bom).is_err(),
            "serde alone should choke, or this test proves nothing"
        );

        let parsed: Appearance = serde_json::from_str(strip_bom(with_bom)).unwrap();
        assert_eq!(parsed.font_size, 20);
    }

    /// A file written by an older build must not lose the fields it does have.
    #[test]
    fn a_partial_file_keeps_its_values_and_defaults_the_rest() {
        let parsed: Appearance = serde_json::from_str(r#"{"fontSize": 18}"#).unwrap();
        assert_eq!(parsed.font_size, 18);
        assert_eq!(parsed.font_family, "Cascadia Mono");
        assert_eq!(parsed.background_opacity, 100);
    }

    #[test]
    fn font_size_is_clamped_to_something_renderable() {
        let tiny = Appearance {
            font_size: 0,
            ..Default::default()
        };
        assert_eq!(tiny.sanitised().font_size, 6);

        let huge = Appearance {
            font_size: 900,
            ..Default::default()
        };
        assert_eq!(huge.sanitised().font_size, 72);
    }

    /// Fully transparent is a state you cannot undo from inside the window.
    #[test]
    fn opacity_never_goes_low_enough_to_lose_the_window() {
        let ghost = Appearance {
            background_opacity: 0,
            ..Default::default()
        };
        assert_eq!(ghost.sanitised().background_opacity, 20);
    }

    #[test]
    fn a_junk_cursor_colour_falls_back_rather_than_reaching_xterm() {
        let bad = Appearance {
            cursor_color: "not a colour".into(),
            ..Default::default()
        };
        assert_eq!(bad.sanitised().cursor_color, "#f5e0dc");

        let short = Appearance {
            cursor_color: "#abc".into(),
            ..Default::default()
        };
        assert_eq!(short.sanitised().cursor_color, "#abc", "#rgb is valid");
    }

    #[test]
    fn an_empty_font_family_falls_back() {
        let blank = Appearance {
            font_family: "   ".into(),
            ..Default::default()
        };
        assert_eq!(blank.sanitised().font_family, "Cascadia Mono");
    }

    #[test]
    fn transparency_is_wanted_for_either_opacity_or_a_material() {
        assert!(!Appearance::default().wants_transparency());

        let dimmed = Appearance {
            background_opacity: 80,
            ..Default::default()
        };
        assert!(dimmed.wants_transparency());

        // Acrylic at full opacity still needs a transparent window to blur through.
        let frosted = Appearance {
            material: Material::Acrylic,
            ..Default::default()
        };
        assert!(frosted.wants_transparency());
    }
}
