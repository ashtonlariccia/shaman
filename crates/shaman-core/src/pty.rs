//! Local PTY sessions, backed by ConPTY on Windows.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

use crate::pump::{self, CoalesceConfig};
use crate::session::{Session, SessionId, SessionKind};
use crate::Result;

/// How to start a local shell.
#[derive(Debug, Clone)]
pub struct PtyOptions {
    pub kind: SessionKind,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub cols: u16,
    pub rows: u16,
    pub elevated: bool,
}

impl PtyOptions {
    /// Launch options for a detected shell.
    pub fn from_profile(profile: &crate::profiles::ShellProfile, cols: u16, rows: u16) -> Self {
        Self {
            kind: profile.kind.clone(),
            program: profile.program.clone(),
            args: profile.args.clone(),
            cwd: None,
            cols,
            rows,
            elevated: false,
        }
    }

    /// A plain `cmd.exe` at the given size.
    pub fn cmd(cols: u16, rows: u16) -> Self {
        Self {
            kind: SessionKind::Cmd,
            program: "cmd.exe".into(),
            args: Vec::new(),
            cwd: None,
            cols,
            rows,
            elevated: false,
        }
    }
}

pub struct PtySession {
    id: SessionId,
    kind: SessionKind,
    elevated: bool,
    master: Box<dyn MasterPty + Send>,
    /// Input is queued rather than written inline; see [`spawn_input_writer`].
    /// Dropping this ends the writer thread, which drops the PTY's writer.
    input: mpsc::Sender<Vec<u8>>,
    /// The child itself is owned by the waiter thread, so only the ability to
    /// kill it is kept here.
    killer: Box<dyn ChildKiller + Send + Sync>,
    /// Dropping this terminates the shell and everything it spawned.
    #[cfg(windows)]
    job: Option<crate::win::Job>,
}

impl PtySession {
    /// Start a shell and stream its output to `on_data`.
    ///
    /// Output is coalesced before it reaches `on_data`, so the callback fires at
    /// a manageable rate even when the shell floods. `on_exit` runs once the PTY
    /// reaches EOF, which is the signal that the process tree is finished.
    /// The caller supplies `id` so it can wire up exit handling that names the
    /// session before the shell has had any chance to die.
    pub fn spawn<D, E>(id: SessionId, opts: PtyOptions, on_data: D, on_exit: E) -> Result<Self>
    where
        D: Fn(Vec<u8>) + Send + 'static,
        E: FnOnce() + Send + 'static,
    {
        // Must happen before portable-pty resolves conpty.dll (it caches the
        // choice in a lazy_static on first use). See `win::harden_dll_search`.
        crate::win::harden_dll_search();

        let pty = native_pty_system();
        let pair = pty.openpty(PtySize {
            rows: opts.rows,
            cols: opts.cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut builder = CommandBuilder::new(&opts.program);
        for arg in &opts.args {
            builder.arg(arg);
        }
        if let Some(cwd) = &opts.cwd {
            builder.cwd(cwd);
        }

        let mut child = pair.slave.spawn_command(builder)?;

        // Contain the shell so its descendants die with it. There is a small
        // window between spawn and assignment in which a grandchild could
        // escape -- ConPTY gives us no way to start the child suspended -- but
        // in practice a shell has not spawned anything by this point.
        #[cfg(windows)]
        let job = match crate::win::Job::new_kill_on_close() {
            Ok(job) => {
                match child.as_raw_handle() {
                    Some(handle) => match job.assign(handle) {
                        Ok(()) => Some(job),
                        Err(err) => {
                            tracing::warn!("could not put shell in job object: {err}");
                            None
                        }
                    },
                    None => {
                        tracing::warn!("shell exposed no process handle; tree kill unavailable");
                        None
                    }
                }
            }
            Err(err) => {
                tracing::warn!("could not create job object: {err}");
                None
            }
        };

        // The slave handle must be dropped, or the PTY never reports EOF when
        // the child exits and the reader thread hangs forever.
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        // Killing is split from waiting so the child can be owned by the waiter
        // thread below while `kill()` still works from here.
        let killer = child.clone_killer();

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        thread::spawn(move || {
            // 64KB rather than 8KB: a flooding shell fills any buffer offered,
            // and each read that comes back is one more allocation, one more
            // channel send, and one more chance for the pump to cut a batch.
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break; // consumer went away
                        }
                    }
                    Err(err) => {
                        tracing::debug!("pty reader stopped: {err}");
                        break;
                    }
                }
            }
            // Dropping tx ends the pump thread, which flushes any tail bytes.
            drop(tx);
        });

        // Exit is detected by WAITING ON THE CHILD, not by the reader hitting
        // EOF. ConPTY keeps the master pipe open for as long as we hold the
        // pseudoconsole, so a shell that runs `exit` leaves the reader blocked
        // forever -- which looked exactly like a hung terminal.
        thread::spawn(move || {
            match child.wait() {
                Ok(status) => tracing::debug!("shell exited: {status:?}"),
                Err(err) => tracing::debug!("waiting on shell failed: {err}"),
            }
            on_exit();
        });

        pump::spawn(rx, CoalesceConfig::default(), on_data);

        Ok(Self {
            id,
            kind: opts.kind,
            elevated: opts.elevated,
            master: pair.master,
            input: spawn_input_writer(id, writer),
            killer,
            #[cfg(windows)]
            job,
        })
    }
}

/// Own the PTY's writer on a thread and feed it from a queue.
///
/// **Why input is queued.** Writing to a ConPTY is writing to a pipe, and a
/// pipe whose reader is not draining it blocks once its buffer fills. The
/// caller here is the UI thread — every keystroke arrives as a synchronous
/// command — so an inline write means one program that has stopped reading its
/// input can freeze the whole window, tabs and menus included. A paste into a
/// program that is busy is enough to do it.
///
/// Queueing also collapses a burst: the loop drains everything already waiting
/// into one buffer, so a pasted block costs a single write and flush rather
/// than one per chunk xterm handed us.
fn spawn_input_writer(id: SessionId, mut writer: Box<dyn Write + Send>) -> mpsc::Sender<Vec<u8>> {
    let (tx, rx) = mpsc::channel::<Vec<u8>>();

    thread::spawn(move || {
        while let Ok(first) = rx.recv() {
            let mut buf = first;
            // Whatever else is already queued belongs in this same write.
            while let Ok(more) = rx.try_recv() {
                buf.extend_from_slice(&more);
            }

            if let Err(err) = writer.write_all(&buf).and_then(|()| writer.flush()) {
                // A shell that has exited takes its input pipe with it, which is
                // ordinary at the end of a session rather than a fault. The tab
                // closes off the child's exit, not off this.
                tracing::debug!("input writer for {id} stopped: {err}");
                break;
            }
        }
    });

    tx
}

impl Session for PtySession {
    fn id(&self) -> SessionId {
        self.id
    }

    fn kind(&self) -> &SessionKind {
        &self.kind
    }

    fn elevated(&self) -> bool {
        self.elevated
    }

    /// Queue input for the shell. Returns as soon as it is queued, so a blocked
    /// or dead reader on the other end cannot stall the caller.
    fn write(&mut self, data: &[u8]) -> Result<()> {
        self.input
            .send(data.to_vec())
            .map_err(|_| crate::Error::Exited)
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    fn kill(&mut self) -> Result<()> {
        // Kill the shell first so it stops producing output, then close the job,
        // which terminates any descendants it left behind.
        let killed = self.killer.kill();

        #[cfg(windows)]
        drop(self.job.take());

        killed?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// Drive a shell through ConPTY until `marker` has been seen `want` times.
    ///
    /// Answers ConPTY's startup cursor-position query, which it blocks on until
    /// a terminal replies -- xterm.js does this for us in the real app.
    fn run_until_marker(
        opts: PtyOptions,
        marker: &str,
        want: usize,
        budget: Duration,
    ) -> std::result::Result<String, String> {
        let out = Arc::new(Mutex::new(Vec::<u8>::new()));
        let sink = Arc::clone(&out);

        let mut session = PtySession::spawn(
            SessionId::next(),
            opts,
            move |chunk| sink.lock().unwrap().extend_from_slice(&chunk),
            || {},
        )
        .map_err(|e| format!("spawn failed: {e}"))?;

        let start = Instant::now();
        let mut answered_dsr = false;
        let mut sent = false;

        loop {
            let seen = String::from_utf8_lossy(&out.lock().unwrap().clone()).to_string();

            if !answered_dsr && seen.contains("\u{1b}[6n") {
                let _ = session.write(b"\x1b[1;1R");
                answered_dsr = true;
            }

            // Give the shell a moment to print its prompt before typing at it.
            if !sent && (answered_dsr || start.elapsed() > Duration::from_secs(3)) {
                let _ = session.write(format!("echo {marker}\r\n").as_bytes());
                sent = true;
            }

            if seen.matches(marker).count() >= want {
                session.kill().ok();
                return Ok(seen);
            }

            if start.elapsed() > budget {
                session.kill().ok();
                return Err(format!("timed out; output was:\n{seen:?}"));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// Every shell the app offers must actually launch and run a command.
    ///
    /// This is the guarantee detection is for: a menu entry that fails on click
    /// is worse than no entry at all.
    #[test]
    fn every_detected_profile_runs_a_command() {
        let profiles = crate::profiles::detect();
        assert!(!profiles.is_empty(), "no shells detected at all");

        for profile in profiles {
            let marker = format!("shaman-{}-ok", profile.id.replace([':', '-'], ""));
            let opts = PtyOptions::from_profile(&profile, 80, 24);

            // WSL may have to boot a VM, so the budget is generous.
            match run_until_marker(opts, &marker, 2, Duration::from_secs(60)) {
                Ok(_) => {}
                Err(err) => panic!("profile '{}' ({}) failed: {err}", profile.id, profile.program),
            }
        }
    }

    /// `exit` must end the session by itself.
    ///
    /// Regression: exit used to be inferred from the reader hitting EOF, but
    /// ConPTY holds the master pipe open while the pseudoconsole lives, so the
    /// callback never fired and the tab hung.
    #[test]
    fn exit_ends_the_session() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let out = Arc::new(Mutex::new(Vec::<u8>::new()));
        let sink = Arc::clone(&out);
        let exited = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&exited);

        let mut session = PtySession::spawn(
            SessionId::next(),
            PtyOptions::cmd(80, 24),
            move |chunk| sink.lock().unwrap().extend_from_slice(&chunk),
            move || flag.store(true, Ordering::SeqCst),
        )
        .expect("spawn cmd.exe");

        let start = Instant::now();
        let mut answered_dsr = false;
        let mut sent = false;

        loop {
            let seen = String::from_utf8_lossy(&out.lock().unwrap().clone()).to_string();

            if !answered_dsr && seen.contains("\u{1b}[6n") {
                session.write(b"\x1b[1;1R").expect("answer cursor report");
                answered_dsr = true;
            }
            if !sent && (answered_dsr || start.elapsed() > Duration::from_secs(3)) {
                session.write(b"exit\r\n").expect("write exit");
                sent = true;
            }
            if exited.load(Ordering::SeqCst) {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "`exit` did not end the session -- the terminal would hang"
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// End-to-end through real ConPTY: run a command, read back its output.
    #[test]
    fn cmd_echoes_back_through_conpty() {
        let out = Arc::new(Mutex::new(Vec::<u8>::new()));
        let sink = Arc::clone(&out);

        let mut session = PtySession::spawn(
            SessionId::next(),
            PtyOptions::cmd(80, 24),
            move |chunk| sink.lock().unwrap().extend_from_slice(&chunk),
            || {},
        )
        .expect("spawn cmd.exe");

        let start = Instant::now();
        let mut answered_dsr = false;
        let mut sent_command = false;

        loop {
            let seen = String::from_utf8_lossy(&out.lock().unwrap().clone()).to_string();

            // ConPTY opens by asking the terminal where the cursor is
            // (`ESC[6n`) and will not proceed until something answers. xterm.js
            // does this automatically; in a test we have to play the terminal.
            if !answered_dsr && seen.contains("\u{1b}[6n") {
                session.write(b"\x1b[1;1R").expect("answer cursor report");
                answered_dsr = true;
            }

            // Don't deadlock the test if a future ConPTY stops asking.
            if !sent_command && (answered_dsr || start.elapsed() > Duration::from_secs(3)) {
                session.write(b"echo conpty-works\r\n").expect("write");
                sent_command = true;
            }

            // Appears twice: once echoed by the terminal, once as output.
            if seen.matches("conpty-works").count() >= 2 {
                break;
            }

            if start.elapsed() > Duration::from_secs(30) {
                session.kill().ok();
                panic!("never saw the marker; got:\n{seen:?}");
            }
            thread::sleep(Duration::from_millis(50));
        }

        session.kill().ok();
    }
}
