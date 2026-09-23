//! Output coalescing.
//!
//! A PTY hands back many tiny writes — a shell printing a build log can produce
//! thousands of reads per second. Forwarding each one across the IPC boundary
//! individually is the single easiest way to lock up the UI, so chunks are
//! batched here before they ever reach the transport.
//!
//! The batching rule is "whichever comes first":
//!
//! * the stream goes quiet for `idle_gap` — the terminal has said what it had
//!   to say, so send it immediately;
//! * `max_delay` elapses — a flood never goes quiet, and a cap on how long a
//!   byte waits is what keeps a busy terminal from feeling laggy;
//! * the buffer reaches `max_bytes`, so a flood cannot grow it without bound.
//!
//! The idle gap is the interesting one. Waiting a fixed delay on every batch
//! taxes the common case — one keystroke echoed back, nothing following it —
//! for the benefit of the rare one. Flushing on quiet instead lets echo leave
//! within a few milliseconds *and* lets `max_delay` be set generously, which is
//! what collapses a build log into roughly one message per rendered frame
//! rather than two or three.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::win::TimerResolution;

#[derive(Debug, Clone, Copy)]
pub struct CoalesceConfig {
    /// Flush once no further bytes have arrived for this long.
    pub idle_gap: Duration,
    /// Longest a byte may wait before being flushed.
    pub max_delay: Duration,
    /// Flush early once the buffer reaches this size.
    pub max_bytes: usize,
}

impl Default for CoalesceConfig {
    fn default() -> Self {
        Self {
            // Short enough to be imperceptible on an echoed keystroke, long
            // enough that the tiny writes ConPTY makes for one redraw (cursor
            // moves, colour changes, the text itself) still land together.
            idle_gap: Duration::from_millis(3),
            // ~One 60Hz frame. Nothing is gained by delivering a flood faster
            // than the renderer can draw it, and every extra message costs a
            // trip across the IPC boundary.
            max_delay: Duration::from_millis(16),
            max_bytes: 64 * 1024,
        }
    }
}

/// Consume `rx`, batching chunks, and hand each batch to `sink`.
///
/// The thread exits once the sender is dropped, flushing whatever it holds.
pub fn spawn<F>(rx: Receiver<Vec<u8>>, cfg: CoalesceConfig, sink: F) -> JoinHandle<()>
where
    F: Fn(Vec<u8>) + Send + 'static,
{
    thread::spawn(move || {
        // The gaps this thread waits out are shorter than Windows' default
        // 15.6ms timer granularity, which would otherwise round every one of
        // them up and undo the pacing below. Held for the life of the pump,
        // released when the session ends.
        let _timer = TimerResolution::acquire();

        // Blocks until there is something to send; an idle terminal costs
        // nothing. Ends when the sender is dropped.
        while let Ok(first) = rx.recv() {
            let mut buf = first;
            let deadline = Instant::now() + cfg.max_delay;
            let mut disconnected = false;

            while buf.len() < cfg.max_bytes {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                // Never wait past the deadline, and never wait longer than the
                // idle gap: a timeout here means the stream went quiet, which
                // is itself a reason to flush.
                let wait = (deadline - now).min(cfg.idle_gap);
                match rx.recv_timeout(wait) {
                    Ok(more) => buf.extend_from_slice(&more),
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }

            sink(buf);

            if disconnected {
                break;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};

    type Batches = Arc<Mutex<Vec<Vec<u8>>>>;

    fn batches() -> Batches {
        Arc::new(Mutex::new(Vec::new()))
    }

    fn sink_into(out: &Batches) -> impl Fn(Vec<u8>) + Send + 'static {
        let out = Arc::clone(out);
        move |b| out.lock().unwrap().push(b)
    }

    #[test]
    fn many_small_writes_collapse_into_few_batches() {
        let (tx, rx) = mpsc::channel();
        let out = batches();
        let handle = spawn(rx, CoalesceConfig::default(), sink_into(&out));

        for _ in 0..500 {
            tx.send(b"x".to_vec()).unwrap();
        }
        drop(tx);
        handle.join().unwrap();

        let batches = out.lock().unwrap();
        let total: usize = batches.iter().map(|b| b.len()).sum();

        assert_eq!(total, 500, "no bytes may be lost");
        assert!(
            batches.len() < 50,
            "expected heavy coalescing, got {} batches",
            batches.len()
        );
    }

    #[test]
    fn flushes_early_once_max_bytes_is_reached() {
        let (tx, rx) = mpsc::channel();
        let out = batches();
        let cfg = CoalesceConfig {
            // Long enough that only the size rule can trigger a flush.
            idle_gap: Duration::from_secs(30),
            max_delay: Duration::from_secs(30),
            max_bytes: 1024,
        };
        let handle = spawn(rx, cfg, sink_into(&out));

        for _ in 0..4 {
            tx.send(vec![b'y'; 512]).unwrap();
        }

        // Must not wait out max_delay: the size threshold governs here.
        let start = Instant::now();
        while out.lock().unwrap().is_empty() {
            assert!(start.elapsed() < Duration::from_secs(5), "never flushed");
            thread::sleep(Duration::from_millis(5));
        }

        drop(tx);
        handle.join().unwrap();
        let total: usize = out.lock().unwrap().iter().map(|b| b.len()).sum();
        assert_eq!(total, 2048);
    }

    /// An echoed keystroke must not wait out `max_delay`.
    ///
    /// This is the rule that keeps typing feeling direct: the shell answers with
    /// a few bytes and then stops, and those bytes should leave as soon as the
    /// stream is quiet rather than sitting until the delay cap expires.
    #[test]
    fn a_quiet_stream_flushes_on_the_idle_gap() {
        let (tx, rx) = mpsc::channel();
        let out = batches();
        let cfg = CoalesceConfig {
            idle_gap: Duration::from_millis(3),
            // So long that a flush can only have come from the idle gap.
            max_delay: Duration::from_secs(30),
            max_bytes: 1024 * 1024,
        };
        let handle = spawn(rx, cfg, sink_into(&out));

        tx.send(b"$ ".to_vec()).unwrap();

        let start = Instant::now();
        while out.lock().unwrap().is_empty() {
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "quiet stream never flushed; it waited for max_delay"
            );
            thread::sleep(Duration::from_millis(1));
        }

        // The sender is still alive, so this was not a shutdown flush.
        assert_eq!(out.lock().unwrap().concat(), b"$ ".to_vec());
        drop(tx);
        handle.join().unwrap();
    }

    #[test]
    fn a_lone_byte_still_arrives() {
        let (tx, rx) = mpsc::channel();
        let out = batches();
        let handle = spawn(rx, CoalesceConfig::default(), sink_into(&out));

        tx.send(b"q".to_vec()).unwrap();
        drop(tx);
        handle.join().unwrap();

        assert_eq!(out.lock().unwrap().concat(), b"q".to_vec());
    }
}
