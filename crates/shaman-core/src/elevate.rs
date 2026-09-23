//! Elevation: knowing whether we're admin, and dropping back down when we are.
//!
//! Shaman's release build runs elevated so admin terminals need no extra
//! prompt. That inverts the usual problem — *ordinary* terminals then have to be
//! pushed back to normal integrity, or every tab would silently be admin.
//!
//! The strategy here was chosen by experiment, not by documentation. The
//! spike that settled it is gone; its results are recorded in `PLAN.md` §4:
//!
//!   * `TokenLinkedToken` — the answer most write-ups give — cannot produce a
//!     primary token without `SeTcbPrivilege`, which admins lack. Fails with
//!     `ERROR_BAD_IMPERSONATION_LEVEL`.
//!   * A SAFER `NORMALUSER` token spawns fine but the child *stays elevated*:
//!     `TokenIsElevated` tracks enabled admin group membership, not integrity.
//!   * `CreateProcessAsUserW` with a borrowed token needs
//!     `SeAssignPrimaryTokenPrivilege` — also not held. `ERROR_PRIVILEGE_NOT_HELD`.
//!
//! What works: duplicate the desktop shell's (explorer.exe) token and launch
//! with `CreateProcessWithTokenW`, which only needs `SeImpersonatePrivilege`.
//!
//! That API ignores `STARTUPINFOEX` attribute lists, so a ConPTY *cannot* be
//! attached to a process launched this way. Hence the helper: we de-elevate a
//! small broker once, and it creates ConPTYs natively at normal integrity.

#[cfg(not(windows))]
mod imp {
    use std::path::Path;

    pub fn is_elevated() -> bool {
        false
    }

    pub fn spawn_deelevated(_exe: &Path, _args: &[String]) -> std::io::Result<u32> {
        Err(std::io::Error::other("de-elevation is Windows-only"))
    }
}

#[cfg(windows)]
mod imp {
    use std::path::Path;

    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        DuplicateTokenEx, GetTokenInformation, SecurityImpersonation, TokenElevation, TokenPrimary,
        TOKEN_ACCESS_MASK, TOKEN_DUPLICATE, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{
        CreateProcessWithTokenW, GetCurrentProcess, OpenProcess, OpenProcessToken,
        CREATE_PROCESS_LOGON_FLAGS, PROCESS_CREATION_FLAGS, PROCESS_INFORMATION,
        PROCESS_QUERY_INFORMATION, STARTUPINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const TOKEN_ALL_ACCESS: u32 = 0x000F_01FF;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Does this process run at high integrity?
    pub fn is_elevated() -> bool {
        unsafe {
            let mut token = HANDLE::default();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
                return false;
            }

            let mut info = TOKEN_ELEVATION::default();
            let mut size = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut info as *mut _ as *mut _),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut size,
            )
            .is_ok();
            CloseHandle(token).ok();

            ok && info.TokenIsElevated != 0
        }
    }

    /// Duplicate the desktop shell's token into a primary token we can launch with.
    ///
    /// Explorer runs as the user at medium integrity, so its token is exactly
    /// the context an ordinary terminal should get.
    fn shell_primary_token() -> std::io::Result<HANDLE> {
        unsafe {
            // Documented way to find the shell without guessing process names.
            let hwnd = GetShellWindow();
            if hwnd.0.is_null() {
                return Err(std::io::Error::other(
                    "no desktop shell window; is Explorer running?",
                ));
            }

            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 {
                return Err(std::io::Error::other(
                    "could not identify the shell process",
                ));
            }

            let shell = OpenProcess(PROCESS_QUERY_INFORMATION, false, pid)
                .map_err(|e| std::io::Error::other(format!("OpenProcess(shell): {e}")))?;

            let mut shell_token = HANDLE::default();
            let opened = OpenProcessToken(shell, TOKEN_DUPLICATE | TOKEN_QUERY, &mut shell_token);
            CloseHandle(shell).ok();
            opened.map_err(|e| std::io::Error::other(format!("OpenProcessToken(shell): {e}")))?;

            let mut primary = HANDLE::default();
            let dup = DuplicateTokenEx(
                shell_token,
                TOKEN_ACCESS_MASK(TOKEN_ALL_ACCESS),
                None,
                SecurityImpersonation,
                TokenPrimary,
                &mut primary,
            );
            CloseHandle(shell_token).ok();
            dup.map_err(|e| std::io::Error::other(format!("DuplicateTokenEx: {e}")))?;

            Ok(primary)
        }
    }

    /// Launch `exe` at *normal* integrity from this (elevated) process.
    ///
    /// Returns the child's process id.
    pub fn spawn_deelevated(exe: &Path, args: &[String]) -> std::io::Result<u32> {
        let token = shell_primary_token()?;

        // CreateProcessWithTokenW wants a mutable command line buffer, and the
        // program path must be quoted in case it contains spaces.
        let mut cmdline = format!("\"{}\"", exe.display());
        for arg in args {
            cmdline.push_str(&format!(" \"{arg}\""));
        }

        let app = wide(&exe.to_string_lossy());
        let mut cmd = wide(&cmdline);

        let si = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut pi = PROCESS_INFORMATION::default();

        let spawned = unsafe {
            CreateProcessWithTokenW(
                token,
                CREATE_PROCESS_LOGON_FLAGS(0),
                PCWSTR(app.as_ptr()),
                Some(PWSTR(cmd.as_mut_ptr())),
                PROCESS_CREATION_FLAGS(CREATE_NO_WINDOW),
                None,
                PCWSTR::null(),
                &si,
                &mut pi,
            )
        };

        unsafe { CloseHandle(token) }.ok();

        spawned.map_err(|e| {
            std::io::Error::other(format!("CreateProcessWithTokenW({}): {e}", exe.display()))
        })?;

        let pid = pi.dwProcessId;
        unsafe {
            CloseHandle(pi.hProcess).ok();
            CloseHandle(pi.hThread).ok();
        }
        Ok(pid)
    }
}

pub use imp::{is_elevated, spawn_deelevated};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevation_query_does_not_panic() {
        // Tests normally run unelevated; the point is that the call is sound and
        // returns something rather than tripping over a bad handle.
        let _ = is_elevated();
    }
}
