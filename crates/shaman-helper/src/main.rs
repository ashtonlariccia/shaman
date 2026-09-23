//! Shaman's medium-integrity terminal broker.
//!
//! Shaman itself runs elevated so admin terminals need no extra UAC prompt.
//! Ordinary terminals must therefore run *lower* than the app that owns them,
//! and a ConPTY cannot be attached to a process launched with
//! `CreateProcessWithTokenW` (that API ignores STARTUPINFOEX attribute lists).
//!
//! So the app de-elevates this small broker once, and the broker creates
//! ConPTYs natively at normal integrity, proxying bytes back over a pipe.
//!
//! **This helper creates the pipe, and the app connects to it.** That direction
//! matters: a pipe created by an elevated process carries a high mandatory
//! label, which would block this medium-integrity process from writing to it.
//! A higher-integrity process can always open a lower-integrity object, so
//! having the lower side listen avoids the problem entirely.
//!
//! Usage (not meant to be run by hand):
//!   shaman-helper.exe <pipe-name> <nonce>

use std::io::Write;
use std::sync::{Arc, Mutex};

use shaman_core::proto::{
    read_frame, write_frame, OpenRequest, ResizeRequest, TAG_DATA, TAG_ERROR, TAG_EXIT, TAG_KILL,
    TAG_OPEN, TAG_READY, TAG_RESIZE, TAG_WRITE,
};
use shaman_core::{PtyOptions, PtySession, Registry, SessionId, SessionKind};

mod pipe;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("SHAMAN_LOG").unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: shaman-helper <pipe-name> <nonce>");
        std::process::exit(2);
    }
    let pipe_name = &args[1];
    let expected_nonce = &args[2];

    // Same DLL hardening as the app: never sideload a conpty.dll off PATH.
    shaman_core::harden_dll_search();

    if let Err(err) = run(pipe_name, expected_nonce) {
        tracing::error!("helper failed: {err}");
        std::process::exit(1);
    }
}

fn run(pipe_name: &str, expected_nonce: &str) -> std::io::Result<()> {
    // TWO pipes, one per direction, on purpose.
    //
    // A single duplex pipe with a `try_clone`d handle deadlocks: the clone
    // duplicates the handle but both refer to the same file object, and Windows
    // serialises I/O on synchronous (non-overlapped) handles. A reader thread
    // parked in ReadFile therefore blocks every concurrent WriteFile forever.
    //
    // Order matters: this side creates and accepts c2s first, so the app must
    // connect to c2s first too.
    tracing::info!("helper listening on {pipe_name}");
    let mut reader = pipe::listen(&format!("{pipe_name}-c2s"))?;
    let writer = Arc::new(Mutex::new(pipe::listen(&format!("{pipe_name}-s2c"))?));

    // Prove to the app that we are the process it launched, not something that
    // guessed the pipe name.
    {
        let mut w = writer.lock().expect("writer lock");
        write_frame(&mut *w, TAG_READY, 0, expected_nonce.as_bytes())?;
    }

    let sessions: Arc<Mutex<Registry>> = Arc::new(Mutex::new(Registry::new()));

    while let Some(frame) = read_frame(&mut reader)? {
        match frame.tag {
            TAG_OPEN => {
                let req: OpenRequest = match serde_json::from_slice(&frame.payload) {
                    Ok(r) => r,
                    Err(e) => {
                        send_error(&writer, frame.id, &format!("bad open request: {e}"));
                        continue;
                    }
                };
                open_session(&sessions, &writer, frame.id, req);
            }

            TAG_WRITE => {
                let mut reg = sessions.lock().expect("registry lock");
                if let Some(session) = reg.get_mut(SessionId(frame.id)) {
                    if let Err(e) = session.write(&frame.payload) {
                        tracing::warn!("write to {} failed: {e}", frame.id);
                    }
                }
            }

            TAG_RESIZE => {
                if let Ok(size) = serde_json::from_slice::<ResizeRequest>(&frame.payload) {
                    let mut reg = sessions.lock().expect("registry lock");
                    if let Some(session) = reg.get_mut(SessionId(frame.id)) {
                        let _ = session.resize(size.cols, size.rows);
                    }
                }
            }

            TAG_KILL => {
                let mut reg = sessions.lock().expect("registry lock");
                if let Some(mut session) = reg.remove(SessionId(frame.id)) {
                    let _ = session.kill();
                }
            }

            other => tracing::warn!("ignoring unknown frame tag {other}"),
        }
    }

    tracing::info!("app disconnected; helper exiting");
    Ok(())
}

fn open_session(
    sessions: &Arc<Mutex<Registry>>,
    writer: &Arc<Mutex<pipe::Stream>>,
    id: u64,
    req: OpenRequest,
) {
    let opts = PtyOptions {
        kind: SessionKind::Custom {
            label: req.program.clone(),
        },
        program: req.program.clone(),
        args: req.args.clone(),
        cwd: req.cwd.clone().map(Into::into),
        cols: req.cols.max(1),
        rows: req.rows.max(1),
        elevated: false,
    };

    let data_writer = Arc::clone(writer);
    let exit_writer = Arc::clone(writer);

    let spawned = PtySession::spawn(
        SessionId(id),
        opts,
        move |chunk| {
            if let Ok(mut w) = data_writer.lock() {
                // A broken pipe just means the app is gone; the read loop will
                // notice and shut us down.
                let _ = write_frame(&mut *w, TAG_DATA, id, &chunk);
            }
        },
        move || {
            if let Ok(mut w) = exit_writer.lock() {
                let _ = write_frame(&mut *w, TAG_EXIT, id, &[]);
            }
        },
    );

    match spawned {
        Ok(session) => {
            sessions
                .lock()
                .expect("registry lock")
                .insert(Box::new(session));
            tracing::info!("opened session {id} ({})", req.program);
        }
        Err(err) => send_error(writer, id, &format!("failed to start {}: {err}", req.program)),
    }
}

fn send_error(writer: &Arc<Mutex<pipe::Stream>>, id: u64, message: &str) {
    tracing::error!("{message}");
    if let Ok(mut w) = writer.lock() {
        let _ = write_frame(&mut *w, TAG_ERROR, id, message.as_bytes());
        let _ = w.flush();
    }
}
