//! Framing for the app <-> helper channel.
//!
//! Deliberately tiny and hand-rolled. Control payloads are JSON (readable when
//! debugging), but terminal output is passed as raw bytes: it is by far the
//! highest-volume traffic and must not pay for base64 or escaping.
//!
//! Frame layout:
//!
//! ```text
//!   u8      tag
//!   u64le   session id
//!   u32le   payload length
//!   bytes   payload
//! ```

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

/// Cap on a single frame, so a desynchronised stream can't trigger a huge
/// allocation.
pub const MAX_FRAME: usize = 8 * 1024 * 1024;

// App -> helper.
pub const TAG_OPEN: u8 = 1;
pub const TAG_WRITE: u8 = 2;
pub const TAG_RESIZE: u8 = 3;
pub const TAG_KILL: u8 = 4;

// Helper -> app.
pub const TAG_DATA: u8 = 10;
pub const TAG_EXIT: u8 = 11;
pub const TAG_ERROR: u8 = 12;
pub const TAG_READY: u8 = 13;

/// Payload of [`TAG_OPEN`], as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenRequest {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub cols: u16,
    pub rows: u16,
}

/// Payload of [`TAG_RESIZE`], as JSON.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ResizeRequest {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub tag: u8,
    pub id: u64,
    pub payload: Vec<u8>,
}

pub fn write_frame(w: &mut impl Write, tag: u8, id: u64, payload: &[u8]) -> io::Result<()> {
    let mut header = [0u8; 13];
    header[0] = tag;
    header[1..9].copy_from_slice(&id.to_le_bytes());
    header[9..13].copy_from_slice(&(payload.len() as u32).to_le_bytes());

    // One write for the header and one for the body; the pipe is buffered on
    // our side, so this does not translate to two syscalls per chunk.
    w.write_all(&header)?;
    w.write_all(payload)?;
    w.flush()
}

pub fn write_json<T: Serialize>(w: &mut impl Write, tag: u8, id: u64, value: &T) -> io::Result<()> {
    let payload = serde_json::to_vec(value).map_err(io::Error::other)?;
    write_frame(w, tag, id, &payload)
}

/// Read one frame. Returns `Ok(None)` on a clean end of stream.
pub fn read_frame(r: &mut impl Read) -> io::Result<Option<Frame>> {
    let mut header = [0u8; 13];
    match r.read_exact(&mut header) {
        Ok(()) => {}
        // The peer closing between frames is normal shutdown, not an error.
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }

    let tag = header[0];
    let id = u64::from_le_bytes(header[1..9].try_into().expect("8 bytes"));
    let len = u32::from_le_bytes(header[9..13].try_into().expect("4 bytes")) as usize;

    if len > MAX_FRAME {
        return Err(io::Error::other(format!(
            "frame of {len} bytes exceeds the {MAX_FRAME} limit; stream is probably desynchronised"
        )));
    }

    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)?;

    Ok(Some(Frame { tag, id, payload }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trips_a_raw_frame() {
        let mut buf = Vec::new();
        write_frame(&mut buf, TAG_DATA, 42, b"hello").unwrap();

        let frame = read_frame(&mut Cursor::new(buf)).unwrap().unwrap();
        assert_eq!(frame.tag, TAG_DATA);
        assert_eq!(frame.id, 42);
        assert_eq!(frame.payload, b"hello");
    }

    #[test]
    fn round_trips_json_control_frames() {
        let req = OpenRequest {
            program: "cmd.exe".into(),
            args: vec!["/k".into()],
            cwd: None,
            cols: 80,
            rows: 24,
        };

        let mut buf = Vec::new();
        write_json(&mut buf, TAG_OPEN, 7, &req).unwrap();

        let frame = read_frame(&mut Cursor::new(buf)).unwrap().unwrap();
        assert_eq!(frame.tag, TAG_OPEN);
        let got: OpenRequest = serde_json::from_slice(&frame.payload).unwrap();
        assert_eq!(got.program, "cmd.exe");
        assert_eq!(got.cols, 80);
    }

    #[test]
    fn reads_several_frames_back_to_back() {
        let mut buf = Vec::new();
        write_frame(&mut buf, TAG_DATA, 1, b"a").unwrap();
        write_frame(&mut buf, TAG_DATA, 2, b"bb").unwrap();
        write_frame(&mut buf, TAG_EXIT, 1, b"").unwrap();

        let mut cursor = Cursor::new(buf);
        let mut seen = Vec::new();
        while let Some(f) = read_frame(&mut cursor).unwrap() {
            seen.push((f.tag, f.id, f.payload.len()));
        }
        assert_eq!(seen, vec![(TAG_DATA, 1, 1), (TAG_DATA, 2, 2), (TAG_EXIT, 1, 0)]);
    }

    #[test]
    fn clean_eof_is_not_an_error() {
        assert!(read_frame(&mut Cursor::new(Vec::new())).unwrap().is_none());
    }

    #[test]
    fn rejects_an_absurd_length_instead_of_allocating() {
        // A desynchronised stream must fail loudly, not try to allocate 4GB.
        let mut header = [0u8; 13];
        header[0] = TAG_DATA;
        header[9..13].copy_from_slice(&u32::MAX.to_le_bytes());

        let err = read_frame(&mut Cursor::new(header.to_vec())).unwrap_err();
        assert!(err.to_string().contains("desynchronised"), "got: {err}");
    }
}
