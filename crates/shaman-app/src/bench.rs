//! Performance harness. Inert unless `SHAMAN_BENCH` is set.
//!
//! Run through `scripts/bench.ps1`, which launches the release build with the
//! variable set, collects the report and closes the app. The frontend half is
//! `ui/src/lib/bench.ts`; between them the pipeline is timed in stages, so a
//! slow number says *where* the time went rather than only that it went:
//!
//! 1. **ConPTY alone** ([`bench_conpty`]): print a file in a cmd.exe hosted
//!    here, counting bytes on the reader thread. No pump, no IPC, no webview --
//!    the ceiling everything downstream is measured against.
//! 2. **The whole pipeline**, in a real tab: keystroke echo latency, and the
//!    same file printed through pump, IPC and xterm.
//! 3. **xterm alone**: the bytes captured in 2, replayed straight into the
//!    terminal, which is its parse and render cost with the shell taken away.
//!
//! The report is logged as `BENCH {json}` and written to `SHAMAN_BENCH_OUT`.

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use shaman_core::{PtyOptions, PtySession, Session, SessionId};
use tauri::AppHandle;

/// The last line of the bench file. Its arrival is how every stage knows the
/// whole file has come through.
const END_MARKER: &str = "SHAMAN_BENCH_END";

fn enabled() -> bool {
    std::env::var_os("SHAMAN_BENCH").is_some()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchConfig {
    profile_id: String,
    file_bytes: u64,
    marker: &'static str,
    /// What to type to print the file. One bulk write from PowerShell rather
    /// than `type`: cmd's `type` writes a line per call, so it measures cmd
    /// rather than the terminal (0.8 MB/s, against ~5 through the same ConPTY).
    command: String,
}

fn print_command(file: &str) -> String {
    format!(
        "powershell -NoProfile -Command \"$o=[Console]::OpenStandardOutput(); \
         $b=[IO.File]::ReadAllBytes('{file}'); $o.Write($b,0,$b.Length); $o.Flush()\""
    )
}

/// An empty round trip, for the fixed cost of one `invoke`.
#[tauri::command]
pub fn bench_ping() {}

/// Stage 2b: the bench file streamed through the real pump and channel with no
/// shell or ConPTY in front of it, as fast as the file can be read. Separates
/// what the IPC path costs a flood from what the programs producing it cost.
#[tauri::command]
pub fn bench_ipc_flood(
    on_output: tauri::ipc::Channel<tauri::ipc::InvokeResponseBody>,
) -> Result<(), String> {
    if !enabled() {
        return Err("benchmarks are off".into());
    }
    let bytes = std::fs::read(std::env::temp_dir().join("shaman-bench.txt"))
        .map_err(|e| format!("bench file: {e}"))?;
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    shaman_core::pump::spawn(rx, shaman_core::CoalesceConfig::default(), move |chunk| {
        let _ = on_output.send(tauri::ipc::InvokeResponseBody::Raw(chunk));
    });
    std::thread::spawn(move || {
        // The size ConPTY's reads come back in under a flood.
        for piece in bytes.chunks(32 * 1024) {
            if tx.send(piece.to_vec()).is_err() {
                break;
            }
        }
    });
    Ok(())
}

/// Output shaped like a build log: short coloured status words, paths, and
/// plain text, ~100 columns a line. Colour matters -- SGR sequences are a real
/// share of what a terminal parses, and a file of bare ASCII flatters it.
fn write_file(path: &PathBuf, megabytes: u64) -> std::io::Result<u64> {
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    let target = megabytes * 1024 * 1024;
    let mut written = 0u64;
    let mut i = 0u64;
    while written < target {
        let line = match i % 4 {
            0 => format!(
                "\x1b[1;32m   Compiling\x1b[0m crate-{i} v0.{}.{} (C:\\src\\workspace\\crates\\crate-{i})\r\n",
                i % 97,
                i % 13
            ),
            1 => format!(
                "\x1b[33mwarning\x1b[0m: unused variable `value_{i}` --> src/lib.rs:{}:{} \x1b[2m(#[warn(unused)] on by default)\x1b[0m\r\n",
                i % 1000,
                i % 80
            ),
            2 => format!(
                "{i:>10} the quick brown fox jumps over the lazy dog 0123456789 abcdefghijklmnopqrstuvwxyz\r\n"
            ),
            _ => format!(
                "\x1b[38;2;137;180;250m{i:08x}\x1b[0m \x1b[38;5;245m│\x1b[0m ok  {} tests passed; 0 failed; finished in 0.{:02}s\r\n",
                i % 500,
                i % 100
            ),
        };
        out.write_all(line.as_bytes())?;
        written += line.len() as u64;
        i += 1;
    }
    let end = format!("{END_MARKER}\r\n");
    out.write_all(end.as_bytes())?;
    out.flush()?;
    Ok(written + end.len() as u64)
}

/// `None` in normal use, which is the frontend's cue to do nothing.
#[tauri::command]
pub fn bench_config() -> Result<Option<BenchConfig>, String> {
    if !enabled() {
        return Ok(None);
    }
    let megabytes = std::env::var("SHAMAN_BENCH_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16);
    let path = std::env::temp_dir().join("shaman-bench.txt");
    let file_bytes = write_file(&path, megabytes).map_err(|e| format!("bench file: {e}"))?;
    Ok(Some(BenchConfig {
        profile_id: "cmd".into(),
        file_bytes,
        marker: END_MARKER,
        command: print_command(&path.display().to_string()),
    }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConptyResult {
    conpty: &'static str,
    ms: f64,
    wire_bytes: u64,
}

/// Stage 1: ConPTY with nothing downstream of it.
#[tauri::command]
pub async fn bench_conpty(command: String, cols: u16, rows: u16) -> Result<ConptyResult, String> {
    if !enabled() {
        return Err("benchmarks are off".into());
    }
    // A PTY blocks and this waits on it; keep both off the async runtime.
    tauri::async_runtime::spawn_blocking(move || conpty_alone(&command, cols, rows))
        .await
        .map_err(|e| e.to_string())?
}

fn conpty_alone(command: &str, cols: u16, rows: u16) -> Result<ConptyResult, String> {
    #[derive(Default)]
    struct Seen {
        bytes: u64,
        /// Enough of the stream's end to find a marker split across reads.
        tail: Vec<u8>,
        asked_position: bool,
        done_at: Option<Instant>,
    }
    let seen = Arc::new(Mutex::new(Seen::default()));
    let sink = Arc::clone(&seen);
    let marker = END_MARKER.as_bytes();

    let mut session = PtySession::spawn(
        SessionId::next(),
        PtyOptions::cmd(cols, rows),
        move |chunk| {
            let mut s = sink.lock().unwrap();
            s.bytes += chunk.len() as u64;
            s.tail.extend_from_slice(&chunk);
            if !s.asked_position && s.tail.windows(4).any(|w| w == b"\x1b[6n") {
                s.asked_position = true;
            }
            if s.done_at.is_none() && s.tail.windows(marker.len()).any(|w| w == marker) {
                s.done_at = Some(Instant::now());
            }
            let keep = s.tail.len().saturating_sub(marker.len() + 8);
            s.tail.drain(..keep);
        },
        || {},
    )
    .map_err(|e| e.to_string())?;

    let conpty = if shaman_core::win::bundled_conpty_loaded() {
        "bundled"
    } else {
        "os"
    };

    // ConPTY opens by asking where the cursor is and waits for an answer; in
    // a tab xterm gives it, here we have to.
    let wait_start = Instant::now();
    while !seen.lock().unwrap().asked_position && wait_start.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(5));
    }
    session.write(b"\x1b[1;1R").map_err(|e| e.to_string())?;
    // Let the banner and prompt land so they are not counted.
    std::thread::sleep(Duration::from_millis(500));
    seen.lock().unwrap().bytes = 0;

    let start = Instant::now();
    session
        .write(format!("{command}\r").as_bytes())
        .map_err(|e| e.to_string())?;

    let done = loop {
        if let Some(at) = seen.lock().unwrap().done_at {
            break at;
        }
        if start.elapsed() > Duration::from_secs(120) {
            session.kill().ok();
            return Err("ConPTY never delivered the end marker".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let wire_bytes = seen.lock().unwrap().bytes;
    session.kill().ok();

    Ok(ConptyResult {
        conpty,
        ms: done.duration_since(start).as_secs_f64() * 1000.0,
        wire_bytes,
    })
}

/// Log the report, write it where the script is waiting for it, and quit.
#[tauri::command]
pub fn bench_report(app: AppHandle, report: serde_json::Value) {
    if !enabled() {
        return;
    }
    let json = report.to_string();
    tracing::info!(target: "shaman::bench", "BENCH {json}");
    if let Some(out) = std::env::var_os("SHAMAN_BENCH_OUT") {
        if let Err(err) = std::fs::write(&out, &json) {
            tracing::error!("could not write the bench report: {err}");
        }
    }
    let _ = std::fs::remove_file(std::env::temp_dir().join("shaman-bench.txt"));
    app.exit(0);
}
