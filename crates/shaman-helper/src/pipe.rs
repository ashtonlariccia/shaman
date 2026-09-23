//! Named-pipe server side.
//!
//! `std` has no named-pipe API, so the handle is created with the Win32 call and
//! then wrapped in a `File` to get `Read`/`Write` for free.

use std::fs::File;
use std::io;

pub type Stream = File;

#[cfg(windows)]
pub fn listen(name: &str) -> io::Result<Stream> {
    use std::os::windows::io::FromRawHandle;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    // PIPE_ACCESS_DUPLEX lives with the file-system flags, not with the pipe APIs.
    use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, NAMED_PIPE_MODE, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
        PIPE_WAIT,
    };

    let path = format!(r"\\.\pipe\{name}");
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

    // 64KiB buffers: terminal output arrives in bursts and a small buffer just
    // means more round trips.
    const BUF: u32 = 64 * 1024;

    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(wide.as_ptr()),
            PIPE_ACCESS_DUPLEX,
            NAMED_PIPE_MODE(PIPE_TYPE_BYTE.0 | PIPE_READMODE_BYTE.0 | PIPE_WAIT.0),
            1, // exactly one client: the app that launched us
            BUF,
            BUF,
            0,
            None,
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }

    // Blocks until the app connects. ERROR_PIPE_CONNECTED (535) means it got in
    // between creation and this call, which is success, not failure.
    let connected = unsafe { ConnectNamedPipe(handle, None) };
    if let Err(err) = connected {
        const ERROR_PIPE_CONNECTED: i32 = 535;
        if err.code().0 & 0xFFFF != ERROR_PIPE_CONNECTED {
            unsafe { CloseHandle(handle) }.ok();
            return Err(io::Error::other(format!("ConnectNamedPipe: {err}")));
        }
    }

    // SAFETY: the handle is valid, owned here, and not closed elsewhere.
    Ok(unsafe { File::from_raw_handle(handle.0 as _) })
}

#[cfg(not(windows))]
pub fn listen(_name: &str) -> io::Result<Stream> {
    Err(io::Error::other("named pipes are Windows-only"))
}
