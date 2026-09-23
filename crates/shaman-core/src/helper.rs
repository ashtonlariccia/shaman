//! Client for the medium-integrity helper.
//!
//! Ordinary terminals cannot be spawned directly by the elevated app (they would
//! inherit its privileges), so they are hosted by `shaman-helper.exe`, which
//! runs at normal integrity. This module launches that helper, connects to it,
//! and presents its sessions as ordinary [`Session`]s.
//!
//! Security notes:
//!
//! * The pipe name and the nonce are both 128 bits of OS randomness. Anything
//!   else on the machine would have to guess the name to squat it.
//! * The helper's first frame must echo the nonce it was launched with. If it
//!   doesn't, something other than our child is on the other end, and we hang
//!   up rather than proxy keystrokes to it.
//! * The helper listens and the app connects, not the other way round: an
//!   elevated process's pipe carries a high mandatory label, which would stop
//!   the medium-integrity helper from writing to it.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::proto::{
    read_frame, write_frame, Frame, OpenRequest, ResizeRequest, TAG_DATA, TAG_ERROR, TAG_EXIT,
    TAG_KILL, TAG_OPEN, TAG_READY, TAG_RESIZE, TAG_WRITE,
};
use crate::session::{Session, SessionId, SessionKind};
use crate::{Error, Result};

type DataSink = Box<dyn Fn(Vec<u8>) + Send>;
type ExitSink = Box<dyn FnOnce() + Send>;

struct Subscriber {
    on_data: DataSink,
    on_exit: Option<ExitSink>,
}

/// A live connection to the helper process.
pub struct Helper {
    /// Frames leave through a queue and a writer thread rather than a locked
    /// handle. Keystrokes are sent from the UI thread, and a `WriteFile` to a
    /// named pipe can block — on the pipe's buffer, or behind another thread
    /// mid-write — which on that thread would be a frozen window. A queue also
    /// makes ordering explicit: frames go out in the order they were produced,
    /// where a mutex only guaranteed whoever won the race went first.
    outbox: mpsc::Sender<Frame>,
    subscribers: Arc<Mutex<HashMap<u64, Subscriber>>>,
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("OS randomness unavailable");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

impl Helper {
    /// Launch the helper de-elevated and connect to it.
    pub fn launch(helper_exe: &Path) -> Result<Arc<Self>> {
        if !helper_exe.is_file() {
            return Err(Error::Other(anyhow::anyhow!(
                "helper not found at {}; ordinary terminals need it when Shaman runs elevated",
                helper_exe.display()
            )));
        }

        let pipe_name = format!("shaman-{}", random_hex(16));
        let nonce = random_hex(16);

        crate::elevate::spawn_deelevated(helper_exe, &[pipe_name.clone(), nonce.clone()])
            .map_err(|e| Error::Other(anyhow::anyhow!("could not start helper: {e}")))?;

        // Two pipes, one per direction. A single duplex pipe deadlocks: a
        // `try_clone`d handle shares one file object, and Windows serialises I/O
        // on synchronous handles, so a reader parked in ReadFile blocks every
        // concurrent WriteFile indefinitely. Connect c2s first -- the helper
        // creates and accepts them in that order.
        let stream = connect_with_retry(&format!("{pipe_name}-c2s"), Duration::from_secs(10))?;
        let mut reader = connect_with_retry(&format!("{pipe_name}-s2c"), Duration::from_secs(10))?;

        // The helper must prove it is ours before we send it anything.
        match read_frame(&mut reader) {
            Ok(Some(frame)) if frame.tag == TAG_READY && frame.payload == nonce.as_bytes() => {}
            Ok(Some(frame)) if frame.tag == TAG_READY => {
                return Err(Error::Other(anyhow::anyhow!(
                    "helper handshake failed: wrong nonce -- refusing to use this pipe"
                )));
            }
            Ok(_) => {
                return Err(Error::Other(anyhow::anyhow!(
                    "helper did not send a handshake"
                )))
            }
            Err(e) => return Err(Error::Other(anyhow::anyhow!("helper handshake: {e}"))),
        }

        let subscribers: Arc<Mutex<HashMap<u64, Subscriber>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let helper = Arc::new(Self {
            outbox: spawn_frame_writer(stream),
            subscribers: Arc::clone(&subscribers),
        });

        thread::spawn(move || {
            loop {
                match read_frame(&mut reader) {
                    Ok(Some(frame)) => dispatch(&subscribers, frame),
                    Ok(None) => break,
                    Err(err) => {
                        tracing::warn!("helper connection lost: {err}");
                        break;
                    }
                }
            }

            // The helper died: every session it hosted is gone with it.
            let mut subs = subscribers.lock().expect("subscriber lock");
            for (_, mut sub) in subs.drain() {
                if let Some(on_exit) = sub.on_exit.take() {
                    on_exit();
                }
            }
        });

        Ok(helper)
    }

    /// Queue one frame for the helper. Returns once queued, not once written.
    fn send(&self, tag: u8, id: u64, payload: Vec<u8>) -> Result<()> {
        self.outbox
            .send(Frame { tag, id, payload })
            .map_err(|_| Error::Other(anyhow::anyhow!("the helper connection is closed")))
    }

    /// Queue a JSON control frame.
    fn send_json<T: serde::Serialize>(&self, tag: u8, id: u64, value: &T) -> Result<()> {
        let payload = serde_json::to_vec(value)
            .map_err(|e| Error::Other(anyhow::anyhow!("could not encode a helper request: {e}")))?;
        self.send(tag, id, payload)
    }

    /// Start a terminal inside the helper.
    pub fn open<D, E>(
        self: &Arc<Self>,
        id: SessionId,
        kind: SessionKind,
        request: OpenRequest,
        on_data: D,
        on_exit: E,
    ) -> Result<HelperSession>
    where
        D: Fn(Vec<u8>) + Send + 'static,
        E: FnOnce() + Send + 'static,
    {
        // Register before sending, or fast output could arrive unrouted.
        self.subscribers.lock().expect("subscriber lock").insert(
            id.0,
            Subscriber {
                on_data: Box::new(on_data),
                on_exit: Some(Box::new(on_exit)),
            },
        );

        self.send_json(TAG_OPEN, id.0, &request)?;

        Ok(HelperSession {
            id,
            kind,
            helper: Arc::clone(self),
        })
    }
}

fn dispatch(subscribers: &Arc<Mutex<HashMap<u64, Subscriber>>>, frame: crate::proto::Frame) {
    match frame.tag {
        TAG_DATA => {
            let subs = subscribers.lock().expect("subscriber lock");
            if let Some(sub) = subs.get(&frame.id) {
                (sub.on_data)(frame.payload);
            }
        }
        TAG_EXIT => {
            let mut subs = subscribers.lock().expect("subscriber lock");
            if let Some(mut sub) = subs.remove(&frame.id) {
                if let Some(on_exit) = sub.on_exit.take() {
                    on_exit();
                }
            }
        }
        TAG_ERROR => {
            let message = String::from_utf8_lossy(&frame.payload).to_string();
            tracing::error!("helper reported error for session {}: {message}", frame.id);

            // Surface it in the terminal itself, then treat the session as dead.
            let mut subs = subscribers.lock().expect("subscriber lock");
            if let Some(mut sub) = subs.remove(&frame.id) {
                (sub.on_data)(format!("\r\n\x1b[31m{message}\x1b[0m\r\n").into_bytes());
                if let Some(on_exit) = sub.on_exit.take() {
                    on_exit();
                }
            }
        }
        other => tracing::warn!("unexpected frame tag {other} from helper"),
    }
}

fn connect_with_retry(pipe_name: &str, budget: Duration) -> Result<File> {
    let path = format!(r"\\.\pipe\{pipe_name}");
    let started = Instant::now();

    loop {
        match OpenOptions::new().read(true).write(true).open(&path) {
            Ok(file) => return Ok(file),
            Err(err) => {
                // The helper may not have created the pipe yet.
                if started.elapsed() > budget {
                    return Err(Error::Other(anyhow::anyhow!(
                        "helper never opened its pipe within {budget:?}: {err}"
                    )));
                }
                thread::sleep(Duration::from_millis(25));
            }
        }
    }
}

/// A terminal that lives inside the helper process.
pub struct HelperSession {
    id: SessionId,
    kind: SessionKind,
    helper: Arc<Helper>,
}

impl Session for HelperSession {
    fn id(&self) -> SessionId {
        self.id
    }

    fn kind(&self) -> &SessionKind {
        &self.kind
    }

    fn elevated(&self) -> bool {
        // The whole point of the helper: these run at normal integrity.
        false
    }

    fn write(&mut self, data: &[u8]) -> Result<()> {
        self.helper.send(TAG_WRITE, self.id.0, data.to_vec())
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.helper
            .send_json(TAG_RESIZE, self.id.0, &ResizeRequest { cols, rows })
    }

    fn kill(&mut self) -> Result<()> {
        self.helper
            .subscribers
            .lock()
            .expect("subscriber lock")
            .remove(&self.id.0);
        self.helper.send(TAG_KILL, self.id.0, Vec::new())
    }
}

impl Drop for HelperSession {
    fn drop(&mut self) {
        // Dropping a tab must not leave a shell running inside the helper.
        let _ = self.helper.send(TAG_KILL, self.id.0, Vec::new());
    }
}

/// Own the pipe and write whatever the queue hands over, in order.
///
/// The thread ends when the last [`Helper`] is dropped, which closes the pipe
/// and is what tells the helper process to exit.
fn spawn_frame_writer(mut stream: File) -> mpsc::Sender<Frame> {
    let (tx, rx) = mpsc::channel::<Frame>();

    thread::spawn(move || {
        while let Ok(frame) = rx.recv() {
            if let Err(err) = write_frame(&mut stream, frame.tag, frame.id, &frame.payload) {
                // The helper is gone. Its read loop noticing the same thing is
                // what ends the sessions it hosted, so there is nothing to do
                // here but stop.
                tracing::warn!("writing to the helper failed: {err}");
                break;
            }
        }
    });

    tx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_values_are_unique_and_long_enough() {
        let a = random_hex(16);
        let b = random_hex(16);
        assert_eq!(a.len(), 32, "128 bits as hex");
        assert_ne!(a, b, "two draws must not collide");
    }

    #[test]
    fn missing_helper_binary_is_a_clear_error() {
        // `Helper` has no Debug impl, so match rather than unwrap_err().
        let err = match Helper::launch(Path::new(r"C:\nope\shaman-helper.exe")) {
            Err(e) => e,
            Ok(_) => panic!("launching a nonexistent helper should fail"),
        };
        assert!(err.to_string().contains("helper not found"), "got: {err}");
    }
}
