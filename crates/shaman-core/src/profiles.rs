//! Detection of the shells actually installed on this machine.
//!
//! Everything here is discovered at runtime. Offering a "PowerShell 7" button
//! that fails on click is worse than not offering it, so a profile only appears
//! if its executable is really present.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::session::SessionKind;

/// A launchable shell.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellProfile {
    /// Stable identifier the frontend passes back to open a session.
    pub id: String,
    pub label: String,
    pub kind: SessionKind,
    pub program: String,
    pub args: Vec<String>,
    /// Launches at high integrity. Shown in red and gated behind UAC.
    pub elevated: bool,
}

fn system_root() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
}

fn exists(path: &Path) -> bool {
    path.is_file()
}

/// Look for an executable in the usual install locations, then on `PATH`.
fn find_program(candidates: &[PathBuf], bare_name: &str) -> Option<PathBuf> {
    for candidate in candidates {
        if exists(candidate) {
            return Some(candidate.clone());
        }
    }

    // PATH fallback, for anything installed somewhere unusual.
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(bare_name))
        .find(|candidate| exists(candidate))
}

/// Every shell available right now, in the order they should appear in the UI.
pub fn detect() -> Vec<ShellProfile> {
    let mut out = Vec::new();
    let root = system_root();

    // cmd.exe is always present on Windows, but resolve it properly anyway
    // rather than relying on PATH.
    let cmd = root.join(r"System32\cmd.exe");
    if exists(&cmd) {
        out.push(ShellProfile {
            id: "cmd".into(),
            label: "Command Prompt".into(),
            kind: SessionKind::Cmd,
            program: cmd.to_string_lossy().into_owned(),
            args: Vec::new(),
            elevated: false,
        });
    }

    // Windows PowerShell 5.1 and PowerShell 7 are different products at
    // different paths; detect them separately rather than assuming.
    let ps = root.join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    if exists(&ps) {
        out.push(ShellProfile {
            id: "powershell".into(),
            label: "Windows PowerShell".into(),
            kind: SessionKind::PowerShell,
            program: ps.to_string_lossy().into_owned(),
            args: vec!["-NoLogo".into()],
            elevated: false,
        });

        // Runs at high integrity. Free of extra prompts because Shaman itself is
        // already elevated; it simply does *not* get de-elevated like the rest.
        out.push(ShellProfile {
            id: "powershell-admin".into(),
            label: "Windows PowerShell (Admin)".into(),
            kind: SessionKind::PowerShell,
            program: ps.to_string_lossy().into_owned(),
            args: vec!["-NoLogo".into()],
            elevated: true,
        });
    }

    let program_files = std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));

    if let Some(pwsh) = find_program(
        &[
            program_files.join(r"PowerShell\7\pwsh.exe"),
            program_files.join(r"PowerShell\6\pwsh.exe"),
        ],
        "pwsh.exe",
    ) {
        out.push(ShellProfile {
            id: "pwsh".into(),
            label: "PowerShell 7".into(),
            kind: SessionKind::Pwsh,
            program: pwsh.to_string_lossy().into_owned(),
            args: vec!["-NoLogo".into()],
            elevated: false,
        });
    }

    let wsl = root.join(r"System32\wsl.exe");
    if exists(&wsl) {
        let distros = wsl_distros();
        // With a single distro the name is noise; only disambiguate when there
        // is actually something to disambiguate.
        let bare = distros.len() == 1;

        for distro in distros {
            out.push(ShellProfile {
                id: format!("wsl:{distro}"),
                label: if bare {
                    "WSL".into()
                } else {
                    format!("WSL · {distro}")
                },
                kind: SessionKind::Wsl {
                    distro: distro.clone(),
                },
                program: wsl.to_string_lossy().into_owned(),
                // Without --cd, WSL inherits the Windows working directory and
                // drops you in /mnt/c/... Start at the Linux home instead.
                args: vec!["-d".into(), distro, "--cd".into(), "~".into()],
                elevated: false,
            });
        }
    }

    out
}

/// Installed WSL distributions.
///
/// Read from the registry rather than by running `wsl.exe -l -q`, whose output
/// is UTF-16LE — decoding it as UTF-8 yields mangled names like `main਀main`.
/// The registry is correctly typed and doesn't require spawning a process.
#[cfg(windows)]
fn wsl_distros() -> Vec<String> {
    use windows_registry::CURRENT_USER;

    const LXSS: &str = r"Software\Microsoft\Windows\CurrentVersion\Lxss";
    /// A distro mid-install or mid-uninstall is not launchable.
    const STATE_INSTALLED: u32 = 1;

    let Ok(lxss) = CURRENT_USER.open(LXSS) else {
        return Vec::new();
    };

    let Ok(keys) = lxss.keys() else {
        return Vec::new();
    };

    let mut distros: Vec<String> = keys
        .filter_map(|guid| {
            let entry = lxss.open(&guid).ok()?;
            if entry.get_u32("State").ok()? != STATE_INSTALLED {
                return None;
            }
            entry.get_string("DistributionName").ok()
        })
        .collect();

    // Registry enumeration order is not meaningful; keep the menu stable.
    distros.sort_unstable();
    distros.dedup();
    distros
}

#[cfg(not(windows))]
fn wsl_distros() -> Vec<String> {
    Vec::new()
}

/// Find a profile by the id the frontend sent back.
pub fn find(id: &str) -> Option<ShellProfile> {
    detect().into_iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn always_finds_cmd() {
        let profiles = detect();
        assert!(
            profiles.iter().any(|p| p.id == "cmd"),
            "cmd.exe must always be detected, got: {:?}",
            profiles.iter().map(|p| &p.id).collect::<Vec<_>>()
        );
    }

    #[test]
    #[cfg(windows)]
    fn every_detected_profile_points_at_a_real_executable() {
        // The whole point of detection: never offer a shell that isn't there.
        for profile in detect() {
            assert!(
                Path::new(&profile.program).is_file(),
                "profile {} points at missing {}",
                profile.id,
                profile.program
            );
        }
    }

    #[test]
    #[cfg(windows)]
    fn profile_ids_are_unique() {
        let profiles = detect();
        let mut ids: Vec<_> = profiles.iter().map(|p| p.id.clone()).collect();
        ids.sort();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate profile ids: {ids:?}");
    }

    #[test]
    #[cfg(windows)]
    fn find_round_trips_detected_ids() {
        for profile in detect() {
            assert!(
                find(&profile.id).is_some(),
                "find failed for {}",
                profile.id
            );
        }
        assert!(find("no-such-shell").is_none());
    }
}
