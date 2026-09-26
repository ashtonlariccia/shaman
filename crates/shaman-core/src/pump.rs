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
//!
//! **A chunk that breaks a silence is not batched at all.** If nothing has
//! arrived for `idle_gap`, whatever came before has already been flushed, and
//! this is most likely the answer to a keystroke -- so it is sent the moment it
//! lands. Measured (`scripts/bench.ps1`), that was most of the echo latency:
//! ConPTY hands back an echoed character in ~0.2ms, and the idle gap was then
//! holding it for 3ms more to see whether anything followed, which for an echo
//! nothing does. It costs no extra messages: a chunk after `idle_gap` of quiet
//! always started a fresh batch anyway, and now that batch simply does not
//! wait. A flood is unaffected -- its reads arrive back to back, so only the
//! first comes after a silence. xterm draws once per frame whatever the message
//! count, so a redraw that happens to be split across two still paints together.

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
            // A backstop, not the usual cut: at ConPTY's ~5MB/s a frame's worth
            // is ~80KB. Each message above 1KB costs the webview an eval plus a
            // fetch, and a flood cut at 64KB stuttered (p95 frame 33ms in
            // scripts/bench.ps1); cutting on time alone brought it to ~20ms.
            max_bytes: 512 * 1024,
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

        // When the last chunk arrived. None until the first, which counts as
        // breaking a silence like any other.
        let mut last_arrival: Option<Instant> = None;

        // Blocks until there is something to send; an idle terminal costs
        // nothing. Ends when the sender is dropped.
        while let Ok(first) = rx.recv() {
            let arrived = Instant::now();
            let after_silence =
                last_arrival.is_none_or(|last| arrived.duration_since(last) >= cfg.idle_gap);
            last_arrival = Some(arrived);
            if after_silence {
                sink(first);
                continue;
            }

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
                    Ok(more) => {
                        last_arrival = Some(Instant::now());
                        buf.extend_from_slice(&more);
                    }
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

        // The first chunk breaks a silence and leaves on its own; the prompt
        // right behind it is the one the idle gap has to release.
        tx.send(b"ls".to_vec()).unwrap();
        tx.send(b"$ ".to_vec()).unwrap();

        let start = Instant::now();
        while out.lock().unwrap().len() < 2 {
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "quiet stream never flushed; it waited for max_delay"
            );
            thread::sleep(Duration::from_millis(1));
        }

        // The sender is still alive, so this was not a shutdown flush.
        assert_eq!(out.lock().unwrap()[1], b"$ ".to_vec());
        drop(tx);
        handle.join().unwrap();
    }

    /// The answer to a keystroke, arriving after a silence, is not held for the
    /// idle gap -- it goes the moment it lands.
    #[test]
    fn a_chunk_after_silence_is_sent_at_once() {
        let (tx, rx) = mpsc::channel();
        let out = batches();
        let cfg = CoalesceConfig {
            idle_gap: Duration::from_millis(50),
            // Long enough that a flush can only come from the idle gap.
            max_delay: Duration::from_secs(30),
            max_bytes: 1024 * 1024,
        };
        let handle = spawn(rx, cfg, sink_into(&out));

        // Batched, "b" would have joined "a" -- it lands well within the gap.
        // Leading-edge, "a" has already gone by the time "b" arrives.
        tx.send(b"a".to_vec()).unwrap();
        tx.send(b"b".to_vec()).unwrap();

        let start = Instant::now();
        while out.lock().unwrap().len() < 2 {
            assert!(start.elapsed() < Duration::from_secs(5), "never flushed");
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(*out.lock().unwrap(), vec![b"a".to_vec(), b"b".to_vec()]);

        drop(tx);
        handle.join().unwrap();
    }

    /// Only a flood's first chunk comes after silence; the rest still batch.
    #[test]
    fn a_flood_after_silence_still_coalesces() {
        let (tx, rx) = mpsc::channel();
        let out = batches();
        let handle = spawn(rx, CoalesceConfig::default(), sink_into(&out));

        for _ in 0..2000 {
            tx.send(vec![b'z'; 32]).unwrap();
        }
        drop(tx);
        handle.join().unwrap();

        let batches = out.lock().unwrap();
        assert_eq!(batches.iter().map(Vec::len).sum::<usize>(), 64_000);
        assert_eq!(batches[0].len(), 32, "the leading chunk goes alone");
        assert!(
            batches.len() < 10,
            "expected coalescing, got {}",
            batches.len()
        );
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
