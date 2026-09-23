//! Proves the elevation split end to end. Run ELEVATED.
//!
//! Two things must both be true for Phase 4 to be correct:
//!
//!   * an admin profile spawned in-process really is **High** integrity, and
//!   * an ordinary profile routed through the helper really is **Medium**.
//!
//! The second is the one that matters: if de-elevation silently failed, every
//! "normal" tab would be an administrator shell while looking ordinary.
//!
//! Writes C:\Users\Public\helper-report.txt.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use shaman_core::proto::OpenRequest;
use shaman_core::{Helper, PtyOptions, PtySession, Session, SessionId, SessionKind};

const REPORT: &str = r"C:\Users\Public\helper-report.txt";

fn helper_exe() -> PathBuf {
    let exe = std::env::current_exe().expect("current exe");
    let dir = exe.parent().expect("exe dir");
    // Examples live in target/<profile>/examples/, the helper one level up.
    let candidates = [
        dir.join("shaman-helper.exe"),
        dir.join("..").join("shaman-helper.exe"),
    ];
    for candidate in candidates {
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from("shaman-helper.exe")
}

/// Drive a terminal until `marker` appears, answering ConPTY's startup cursor
/// query (which it blocks on until a terminal replies).
fn drive(
    write: &mut dyn FnMut(&[u8]),
    output: &Arc<Mutex<Vec<u8>>>,
    command: &str,
    marker: &str,
    budget: Duration,
) -> Result<String, String> {
    let start = Instant::now();
    let mut answered_dsr = false;
    let mut sent = false;

    loop {
        let seen = String::from_utf8_lossy(&output.lock().unwrap().clone()).to_string();

        if !answered_dsr && seen.contains("\u{1b}[6n") {
            write(b"\x1b[1;1R");
            answered_dsr = true;
        }
        if !sent && (answered_dsr || start.elapsed() > Duration::from_secs(3)) {
            write(format!("{command}\r\n").as_bytes());
            sent = true;
        }
        if seen.contains(marker) {
            return Ok(seen);
        }
        if start.elapsed() > budget {
            return Err(format!("timed out waiting for {marker:?}; saw:\n{seen}"));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// `whoami /groups` prints the integrity level, which is the ground truth.
fn integrity_of(seen: &str) -> &'static str {
    if seen.contains("High Mandatory Level") {
        "HIGH"
    } else if seen.contains("Medium Mandatory Level") {
        "MEDIUM"
    } else {
        "UNKNOWN"
    }
}

fn main() {
    // Written after every line, not at the end: the first attempt hung and left
    // no report at all, so there was no way to tell how far it had got.
    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    macro_rules! say {
        ($($arg:tt)*) => {{
            let line = format!($($arg)*);
            println!("{line}");
            let mut l = log.lock().unwrap();
            l.push(line);
            let _ = std::fs::write(REPORT, l.join("\r\n"));
        }};
    }

    shaman_core::harden_dll_search();

    let elevated = shaman_core::is_elevated();
    say!("=== helper / elevation spike ===");
    say!("app elevated: {elevated}");

    if !elevated {
        say!("");
        say!("NOT ELEVATED — run this elevated or the split cannot be tested.");
        return;
    }

    // --- 1. Admin profile, spawned in-process -------------------------------
    say!("");
    say!("--- in-process (admin tab) ---");
    say!("spawning in-process cmd...");
    let admin_out = Arc::new(Mutex::new(Vec::<u8>::new()));
    let sink = Arc::clone(&admin_out);

    let admin_result = PtySession::spawn(
        SessionId::next(),
        PtyOptions::cmd(120, 30),
        move |chunk| sink.lock().unwrap().extend_from_slice(&chunk),
        || {},
    );

    match admin_result {
        Ok(mut session) => {
            let mut write = |bytes: &[u8]| {
                let _ = session.write(bytes);
            };
            match drive(
                &mut write,
                &admin_out,
                "whoami /groups | findstr Mandatory",
                "Mandatory Level",
                Duration::from_secs(30),
            ) {
                Ok(seen) => say!("in-process integrity: {}", integrity_of(&seen)),
                Err(e) => say!("in-process FAILED: {e}"),
            }
            let _ = session.kill();
        }
        Err(e) => say!("in-process spawn FAILED: {e}"),
    }

    // --- 2. Ordinary profile, routed through the helper ---------------------
    say!("");
    say!("--- via helper (normal tab) ---");
    let path = helper_exe();
    say!("helper binary: {}", path.display());

    say!("launching helper (de-elevated)...");
    match Helper::launch(&path) {
        Ok(helper) => {
            say!("helper connected; opening session...");
            let normal_out = Arc::new(Mutex::new(Vec::<u8>::new()));
            let sink = Arc::clone(&normal_out);

            let opened = helper.open(
                SessionId::next(),
                SessionKind::Cmd,
                OpenRequest {
                    program: r"C:\Windows\System32\cmd.exe".into(),
                    args: vec![],
                    cwd: None,
                    cols: 120,
                    rows: 30,
                },
                move |chunk| sink.lock().unwrap().extend_from_slice(&chunk),
                || {},
            );

            match opened {
                Ok(mut session) => {
                    say!("session opened; driving...");
                    let mut write = |bytes: &[u8]| {
                        let _ = session.write(bytes);
                    };
                    match drive(
                        &mut write,
                        &normal_out,
                        "whoami /groups | findstr Mandatory",
                        "Mandatory Level",
                        Duration::from_secs(40),
                    ) {
                        Ok(seen) => say!("helper integrity: {}", integrity_of(&seen)),
                        Err(e) => say!("helper session FAILED: {e}"),
                    }
                    let _ = session.kill();
                }
                Err(e) => say!("helper open FAILED: {e}"),
            }
        }
        Err(e) => say!("helper launch FAILED: {e}"),
    }

    say!("");
    say!("EXPECTED: in-process = HIGH, helper = MEDIUM");
}
