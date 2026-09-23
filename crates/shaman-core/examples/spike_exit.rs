//! Does `exit` in a shell actually end the session?
//!
//! Two paths must both fire their exit callback:
//!
//!   1. in-process `PtySession` (admin tabs, and every tab in a dev build)
//!   2. a session hosted by `shaman-helper` (ordinary tabs in a release build)
//!
//! The UI closes a tab when that callback reaches it, so a callback that never
//! fires looks exactly like a hung terminal.
//!
//! Writes C:\Users\Public\exit-report.txt.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use shaman_core::proto::OpenRequest;
use shaman_core::{Helper, PtyOptions, PtySession, Session, SessionId, SessionKind};

const REPORT: &str = r"C:\Users\Public\exit-report.txt";
const BUDGET: Duration = Duration::from_secs(20);

fn helper_exe() -> PathBuf {
    let exe = std::env::current_exe().expect("current exe");
    let dir = exe.parent().expect("exe dir");
    for candidate in [
        dir.join("shaman-helper.exe"),
        dir.join("..").join("shaman-helper.exe"),
    ] {
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from("shaman-helper.exe")
}

/// Drive a shell to a prompt, then send `exit`, and report whether the exit
/// callback fired.
fn exercise(
    label: &str,
    log: &Arc<Mutex<Vec<String>>>,
    output: Arc<Mutex<Vec<u8>>>,
    exited: Arc<AtomicBool>,
    session: &mut dyn Session,
) {
    let say = |line: String| {
        println!("{line}");
        let mut l = log.lock().unwrap();
        l.push(line);
        let _ = std::fs::write(REPORT, l.join("\r\n"));
    };

    let start = Instant::now();
    let mut answered_dsr = false;
    let mut sent_exit = false;

    loop {
        let seen = String::from_utf8_lossy(&output.lock().unwrap().clone()).to_string();

        // ConPTY blocks until a terminal answers its cursor query.
        if !answered_dsr && seen.contains("\u{1b}[6n") {
            let _ = session.write(b"\x1b[1;1R");
            answered_dsr = true;
        }

        if !sent_exit && (answered_dsr || start.elapsed() > Duration::from_secs(3)) {
            let _ = session.write(b"exit\r\n");
            sent_exit = true;
            say(format!("{label}: sent `exit`"));
        }

        if exited.load(Ordering::SeqCst) {
            say(format!(
                "{label}: PASS — exit callback fired after {:?}",
                start.elapsed()
            ));
            return;
        }

        if start.elapsed() > BUDGET {
            say(format!(
                "{label}: FAIL — no exit callback within {BUDGET:?} (this is the hang)"
            ));
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn main() {
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
    say!("=== exit behaviour ===");
    say!("elevated: {}", shaman_core::is_elevated());

    // --- 1. in-process -------------------------------------------------------
    say!("");
    say!("--- in-process PtySession ---");
    {
        let output = Arc::new(Mutex::new(Vec::<u8>::new()));
        let sink = Arc::clone(&output);
        let exited = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&exited);

        match PtySession::spawn(
            SessionId::next(),
            PtyOptions::cmd(80, 24),
            move |chunk| sink.lock().unwrap().extend_from_slice(&chunk),
            move || flag.store(true, Ordering::SeqCst),
        ) {
            Ok(mut session) => {
                exercise("in-process", &log, output, exited, &mut session);
                let _ = session.kill();
            }
            Err(e) => say!("in-process: spawn failed: {e}"),
        }
    }

    // --- 2. via the helper ---------------------------------------------------
    say!("");
    say!("--- helper-hosted session ---");
    let path = helper_exe();
    match Helper::launch(&path) {
        Ok(helper) => {
            let output = Arc::new(Mutex::new(Vec::<u8>::new()));
            let sink = Arc::clone(&output);
            let exited = Arc::new(AtomicBool::new(false));
            let flag = Arc::clone(&exited);

            match helper.open(
                SessionId::next(),
                SessionKind::Cmd,
                OpenRequest {
                    program: r"C:\Windows\System32\cmd.exe".into(),
                    args: vec![],
                    cwd: None,
                    cols: 80,
                    rows: 24,
                },
                move |chunk| sink.lock().unwrap().extend_from_slice(&chunk),
                move || flag.store(true, Ordering::SeqCst),
            ) {
                Ok(mut session) => {
                    exercise("helper", &log, output, exited, &mut session);
                    let _ = session.kill();
                }
                Err(e) => say!("helper: open failed: {e}"),
            }
        }
        Err(e) => say!("helper: launch failed: {e} (needs an elevated run)"),
    }

    say!("");
    say!("done");
}
