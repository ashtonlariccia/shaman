//! Windows process hardening.

#[cfg(windows)]
mod imp {
    use std::sync::Once;

    // kernel32.lib is linked by default under MSVC. Declared by hand to avoid
    // pulling in the `windows` crate this early; Phase 4 brings it in properly
    // for token and named-pipe work.
    extern "system" {
        fn SetDefaultDllDirectories(directory_flags: u32) -> i32;
    }

    /// Application directory + System32 + any explicitly added directories.
    /// Critically, this excludes the current directory and everything on `PATH`.
    const LOAD_LIBRARY_SEARCH_DEFAULT_DIRS: u32 = 0x0000_1000;

    static ONCE: Once = Once::new();

    /// Remove `PATH` and the current directory from the DLL search order.
    ///
    /// This exists because of a concrete, reproducible failure: `portable-pty`
    /// deliberately prefers a *sideloaded* `conpty.dll` over the one exported by
    /// `kernel32`, loading it by bare name:
    ///
    /// ```text
    /// if let Ok(sideloaded) = ConPtyFuncs::open(Path::new("conpty.dll")) { ... }
    /// ```
    ///
    /// A bare name goes through the default DLL search order, which includes
    /// `PATH`. WezTerm installs to a directory that is on `PATH` and ships both
    /// `conpty.dll` and `OpenConsole.exe`. The result: we silently ran someone
    /// else's console host, and spawning a shell hung forever at 0% CPU with no
    /// error — one of the least debuggable failure modes available.
    ///
    /// Restricting the search order pins us to the OS ConPTY in `kernel32`.
    ///
    /// This also closes a genuine DLL-planting hole. That matters more than
    /// usual here, because Shaman is intended to run elevated.
    ///
    /// Call this before any PTY is created. It is idempotent.
    pub fn harden_dll_search() {
        ONCE.call_once(|| {
            // SAFETY: plain kernel32 call, no pointers, safe to call any time.
            let ok = unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS) };
            if ok == 0 {
                tracing::warn!(
                    "SetDefaultDllDirectories failed; a conpty.dll on PATH could be sideloaded"
                );
            } else {
                tracing::debug!("DLL search order restricted to app dir + System32");
            }
        });
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn harden_dll_search() {}
}

pub use imp::harden_dll_search;

/// Fine-grained waits, for as long as a guard is held.
#[cfg(windows)]
mod timer {
    use std::sync::Mutex;

    // winmm.lib ships with the SDK; declared by hand rather than growing the
    // `windows` crate's feature list for two calls with no pointers in them.
    #[link(name = "winmm")]
    extern "system" {
        fn timeBeginPeriod(period: u32) -> u32;
        fn timeEndPeriod(period: u32) -> u32;
    }

    const MILLISECOND: u32 = 1;
    const TIMERR_NOERROR: u32 = 0;

    /// How many guards are alive. The first one raises the resolution and the
    /// last one drops it, so a machine with no terminals open pays nothing.
    static HOLDERS: Mutex<u32> = Mutex::new(0);

    /// A raised system timer resolution, released on drop.
    ///
    /// **Why this exists.** Windows' default timer granularity is 15.6ms, and
    /// waits round *up* to it: a thread asking for 8ms sleeps for 15.6. The
    /// output pump is built on exactly that kind of short wait, so without this
    /// its batches land on a 15.6ms grid instead of the 3-16ms one it asks for
    /// — which is visible as choppy, lumpy output when a command prints
    /// steadily, and as a wobble between fast and slow echo while typing.
    ///
    /// Windows 11 made the resolution *per-process*, so nothing else on the
    /// machine can raise it on our behalf any more — notably not WebView2,
    /// which renders in its own process. We have to ask for ourselves.
    ///
    /// The cost is a slightly busier scheduler while a terminal is open, which
    /// is the correct trade for an interactive foreground app, and it is given
    /// back the moment the last session closes.
    #[derive(Debug)]
    pub struct TimerResolution {
        // Not `Copy`/`Clone`: releasing must happen exactly once per guard.
        _private: (),
    }

    impl TimerResolution {
        /// Raise the timer resolution to 1ms until the guard is dropped.
        pub fn acquire() -> Self {
            let mut holders = HOLDERS.lock().unwrap_or_else(|e| e.into_inner());
            if *holders == 0 {
                // SAFETY: plain winmm call with a scalar argument.
                let rc = unsafe { timeBeginPeriod(MILLISECOND) };
                if rc == TIMERR_NOERROR {
                    tracing::debug!("timer resolution raised to {MILLISECOND}ms");
                } else {
                    tracing::warn!(
                        "timeBeginPeriod({MILLISECOND}) refused (rc={rc}); \
                         short waits will round up to the default granularity"
                    );
                }
            }
            *holders += 1;
            Self { _private: () }
        }
    }

    impl Drop for TimerResolution {
        fn drop(&mut self) {
            let mut holders = HOLDERS.lock().unwrap_or_else(|e| e.into_inner());
            *holders = holders.saturating_sub(1);
            if *holders == 0 {
                // SAFETY: balanced against the timeBeginPeriod above; Windows
                // requires one end call per begin call.
                let _ = unsafe { timeEndPeriod(MILLISECOND) };
                tracing::debug!("timer resolution released");
            }
        }
    }
}

#[cfg(not(windows))]
mod timer {
    /// No-op elsewhere: POSIX timers are already fine-grained.
    #[derive(Debug)]
    pub struct TimerResolution {
        _private: (),
    }

    impl TimerResolution {
        pub fn acquire() -> Self {
            Self { _private: () }
        }
    }
}

pub use timer::TimerResolution;

/// Where the pointer is, and what is under it.
///
/// Dragging a terminal out of the sidebar has to answer one question at the
/// moment the button comes up: which window — if any — was the cursor over?
/// The webview cannot answer it. It holds the mouse capture for the duration of
/// the drag, so its own pointer events keep arriving no matter where the cursor
/// actually is, and it has no idea what sits above or beside it on the desktop.
/// Asking the OS is the only honest answer, and it costs two calls.
#[cfg(windows)]
mod desktop {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetAncestor, GetCursorPos, WindowFromPoint, GA_ROOT,
    };

    /// The cursor, in physical screen pixels.
    ///
    /// Physical rather than the CSS pixels the webview deals in, deliberately:
    /// on a multi-monitor setup the two scale differently, and converting in
    /// the frontend would put a DPI guess between the pointer and the drop.
    pub fn cursor_position() -> Option<(i32, i32)> {
        let mut point = POINT::default();
        // SAFETY: writes through a pointer to a local that outlives the call.
        unsafe { GetCursorPos(&mut point) }.ok()?;
        Some((point.x, point.y))
    }

    /// The top-level window under a screen point, as a raw `HWND` value.
    ///
    /// `WindowFromPoint` reports the deepest child it can find — for a Tauri
    /// window that is the WebView2 render surface, not the window the app knows
    /// by label — so the result is walked back up to its root before being
    /// handed out. Returned as an `isize` so callers can compare it against
    /// their own window handles without this crate taking a dependency on the
    /// exact `windows` version they were built with.
    pub fn root_window_at(x: i32, y: i32) -> Option<isize> {
        // SAFETY: both calls take plain values and return a handle we only
        // ever compare; no ownership is implied by either.
        let hit = unsafe { WindowFromPoint(POINT { x, y }) };
        if hit.0.is_null() {
            return None;
        }
        let root = unsafe { GetAncestor(hit, GA_ROOT) };
        Some(if root.0.is_null() { hit.0 } else { root.0 } as isize)
    }
}

#[cfg(not(windows))]
mod desktop {
    pub fn cursor_position() -> Option<(i32, i32)> {
        None
    }
    pub fn root_window_at(_x: i32, _y: i32) -> Option<isize> {
        None
    }
}

pub use desktop::{cursor_position, root_window_at};

#[cfg(windows)]
mod job {
    use std::os::windows::io::RawHandle;

    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// A Windows job object that kills every process inside it when closed.
    ///
    /// Killing a shell does **not** kill what the shell started: `taskkill` on
    /// `cmd.exe` happily orphans a running `ping`, and those orphans keep the
    /// PTY's pipes open. Putting the shell in a job with
    /// `KILL_ON_JOB_CLOSE` makes the whole process tree die with the tab, which
    /// is what closing a terminal is supposed to mean.
    #[derive(Debug)]
    pub struct Job(HANDLE);

    // The handle is owned exclusively by this struct.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        /// Create a job whose members are terminated when it is dropped.
        pub fn new_kill_on_close() -> std::io::Result<Self> {
            // SAFETY: null attributes/name is the documented default form.
            let handle = unsafe { CreateJobObjectW(None, None) }?;

            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

            // SAFETY: `info` matches the class being set and outlives the call.
            unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            }?;

            Ok(Self(handle))
        }

        /// Put a process — and by inheritance everything it spawns — in the job.
        pub fn assign(&self, process: RawHandle) -> std::io::Result<()> {
            // SAFETY: caller supplies a live process handle.
            unsafe { AssignProcessToJobObject(self.0, HANDLE(process as _)) }?;
            Ok(())
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // Closing the last handle is what triggers the kill.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(windows)]
pub use job::Job;

#[cfg(all(test, windows))]
mod tests {
    use super::{Job, TimerResolution};
    use std::os::windows::io::AsRawHandle;
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    /// Nested guards must not release the resolution early, and a short wait
    /// must actually be short while one is held.
    ///
    /// The threshold is 10ms: the default granularity is 15.6ms, so a 2ms wait
    /// that returns in under 10 proves the request took effect. It is a real
    /// timing assertion, but a forgiving one — the failure it guards against is
    /// a 7x overshoot.
    #[test]
    fn a_held_guard_makes_short_waits_short() {
        let outer = TimerResolution::acquire();
        let inner = TimerResolution::acquire();
        drop(inner); // must not release while `outer` lives

        let start = Instant::now();
        thread::sleep(Duration::from_millis(2));
        let slept = start.elapsed();

        drop(outer);
        assert!(
            slept < Duration::from_millis(10),
            "a 2ms sleep took {slept:?}; the timer resolution was not raised"
        );
    }

    /// The whole point of the job object: closing it must kill what is inside,
    /// with no explicit kill call on the process itself.
    #[test]
    fn closing_a_job_kills_its_processes() {
        let job = Job::new_kill_on_close().expect("create job");

        // Long-running and harmless.
        let mut child = Command::new("ping.exe")
            .args(["-n", "300", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn ping");

        job.assign(child.as_raw_handle()).expect("assign to job");

        assert!(
            child.try_wait().expect("try_wait").is_none(),
            "child should still be running before the job closes"
        );

        drop(job);

        let start = Instant::now();
        loop {
            if child.try_wait().expect("try_wait").is_some() {
                break;
            }
            if start.elapsed() > Duration::from_secs(10) {
                let _ = child.kill();
                panic!("closing the job did not kill the process");
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
}
