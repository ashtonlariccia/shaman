//! SPIKE: can an elevated process spawn a *non*-elevated child?
//!
//! This is the load-bearing assumption of Shaman's elevation model. The app
//! runs elevated so admin tabs are seamless, which means every ordinary tab has
//! to be pushed back down to normal integrity. If this can't be done, the model
//! needs rethinking — so it gets proven before anything is built on it.
//!
//! Three strategies are tried in one run, because each has a plausible failure
//! mode and a single elevated run is cheap while a UAC prompt is not:
//!
//!   1. linked token  — the "textbook" answer, and the one that fails here:
//!      `TokenLinkedToken` only yields a primary-capable token to a caller
//!      holding SeTcbPrivilege, which administrators do not have (SYSTEM does).
//!      Without it the token is identification-level and cannot be duplicated
//!      to primary: ERROR_BAD_IMPERSONATION_LEVEL (0x80070542).
//!
//!   2. shell token   — borrow the token from the process owning the shell
//!      window (explorer.exe), which already runs at medium integrity as the
//!      user. Widely used; depends on Explorer running.
//!
//!   3. SAFER         — derive a restricted NORMALUSER token from our own and
//!      explicitly stamp it with the medium integrity level. Depends on no
//!      other process.
//!
//! Run elevated. It writes C:\Users\Public\spike-report.txt.

#[cfg(not(windows))]
fn main() {
    eprintln!("windows only");
}

#[cfg(windows)]
fn main() {
    use std::env;

    let args: Vec<String> = env::args().collect();

    // Child mode: report our own elevation + integrity, then exit.
    if args.len() >= 3 && args[1] == "child" {
        let elevated = win::is_elevated().unwrap_or(false);
        let _ = std::fs::write(&args[2], if elevated { "ELEVATED" } else { "NORMAL" });
        return;
    }

    // The spike writes its own transcript: an elevated Start-Process cannot
    // redirect output, and wrapping it in cmd.exe to redirect drags in a layer
    // of quote-escaping that already broke this once.
    const REPORT: &str = r"C:\Users\Public\spike-report.txt";
    let mut log: Vec<String> = Vec::new();

    macro_rules! say {
        ($($arg:tt)*) => {{
            let line = format!($($arg)*);
            println!("{line}");
            log.push(line);
        }};
    }

    say!("=== de-elevation spike ===");

    let parent_elevated = win::is_elevated().unwrap_or(false);
    say!("parent elevated: {parent_elevated}");

    if !parent_elevated {
        say!("");
        say!("NOT RUNNING ELEVATED — meaningless unless the parent is elevated.");
        let _ = std::fs::write(REPORT, log.join("\r\n"));
        return;
    }

    let exe = env::current_exe().expect("current exe");
    let mut results: Vec<(&str, String)> = Vec::new();

    type Strategy = fn(&std::path::Path, &str) -> windows::core::Result<()>;
    let strategies: [(&str, Strategy); 4] = [
        ("linked token", win::spawn_via_linked_token),
        ("shell token", win::spawn_via_shell_token),
        ("SAFER restricted", win::spawn_via_safer),
        // If this passes, de-elevated ConPTYs work in-process and no helper
        // process is needed.
        ("shell+AsUser", win::spawn_via_shell_token_as_user),
    ];

    for (name, spawn) in strategies {
        say!("");
        say!("--- strategy: {name} ---");

        let out_path = env::temp_dir().join(format!("shaman-spike-{}.txt", name.replace(' ', "-")));
        let _ = std::fs::remove_file(&out_path);
        let cmdline = format!("\"{}\" child \"{}\"", exe.display(), out_path.display());

        match spawn(&exe, &cmdline) {
            Ok(()) => {
                let verdict = std::fs::read_to_string(&out_path)
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|e| format!("<no output: {e}>"));
                say!("child reported: {verdict}");
                results.push((name, verdict));
            }
            Err(err) => {
                say!("spawn failed: {err}");
                results.push((name, format!("SPAWN FAILED {:?}", err.code())));
            }
        }
    }

    say!("");
    say!("=== summary ===");
    let mut winner = None;
    for (name, verdict) in &results {
        let ok = verdict == "NORMAL";
        say!("  {:<18} {}", name, if ok { "PASS" } else { verdict });
        if ok && winner.is_none() {
            winner = Some(*name);
        }
    }

    say!("");
    match winner {
        Some(name) => say!("RESULT: PASS — use '{name}'. Elevation model is viable."),
        None => say!("RESULT: FAIL — no strategy produced a non-elevated child."),
    }

    let _ = std::fs::write(REPORT, log.join("\r\n"));
}

#[cfg(windows)]
mod win {
    use std::path::Path;

    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::AppLocker::{
        SaferCloseLevel, SaferComputeTokenFromLevel, SaferCreateLevel,
        SAFER_COMPUTE_TOKEN_FROM_LEVEL_FLAGS, SAFER_LEVELID_NORMALUSER, SAFER_LEVEL_OPEN,
        SAFER_SCOPEID_USER,
    };
    use windows::Win32::Security::Authorization::ConvertStringSidToSidW;
    use windows::Win32::Security::{
        DuplicateTokenEx, GetTokenInformation, SetTokenInformation, SecurityImpersonation,
        TokenElevation, TokenIntegrityLevel, TokenLinkedToken, TokenPrimary, PSID,
        SAFER_LEVEL_HANDLE, SID_AND_ATTRIBUTES, TOKEN_ACCESS_MASK, TOKEN_DUPLICATE, TOKEN_ELEVATION,
        TOKEN_LINKED_TOKEN, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{
        CreateProcessAsUserW, CreateProcessWithTokenW, GetCurrentProcess, OpenProcess,
        OpenProcessToken, WaitForSingleObject, CREATE_PROCESS_LOGON_FLAGS, INFINITE,
        PROCESS_CREATION_FLAGS, PROCESS_INFORMATION, PROCESS_QUERY_INFORMATION, STARTUPINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const TOKEN_ALL_ACCESS: u32 = 0x000F_01FF;
    const SE_GROUP_INTEGRITY: u32 = 0x0000_0020;
    /// S-1-16-8192 — the medium (normal user) integrity level.
    const MEDIUM_INTEGRITY_SID: &str = "S-1-16-8192";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn current_token() -> windows::core::Result<HANDLE> {
        let mut token = HANDLE::default();
        unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_QUERY | TOKEN_DUPLICATE,
                &mut token,
            )
        }?;
        Ok(token)
    }

    pub fn is_elevated() -> windows::core::Result<bool> {
        let token = current_token()?;
        let mut info = TOKEN_ELEVATION::default();
        let mut size = 0u32;

        let result = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut info as *mut _ as *mut _),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut size,
            )
        };
        unsafe { CloseHandle(token) }.ok();
        result?;

        Ok(info.TokenIsElevated != 0)
    }

    /// Launch with an explicit primary token, via CreateProcessWithTokenW.
    ///
    /// NOTE for the real implementation: this API silently ignores STARTUPINFOEX
    /// attribute lists, so a ConPTY pseudoconsole cannot be attached here. That
    /// is exactly why the design spawns a de-elevated *helper*, which then
    /// creates its own ConPTYs at normal integrity.
    fn launch_with_token(token: HANDLE, exe: &Path, cmdline: &str) -> windows::core::Result<()> {
        let si = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut pi = PROCESS_INFORMATION::default();

        let app = wide(&exe.to_string_lossy());
        let mut cmd = wide(cmdline);

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

        if spawned.is_ok() {
            unsafe {
                WaitForSingleObject(pi.hProcess, INFINITE);
                CloseHandle(pi.hProcess).ok();
                CloseHandle(pi.hThread).ok();
            }
        }
        spawned
    }

    /// Strategy 1: the elevated token's linked "filtered" companion.
    pub fn spawn_via_linked_token(exe: &Path, cmdline: &str) -> windows::core::Result<()> {
        let token = current_token()?;

        let mut linked = TOKEN_LINKED_TOKEN::default();
        let mut size = 0u32;
        let got = unsafe {
            GetTokenInformation(
                token,
                TokenLinkedToken,
                Some(&mut linked as *mut _ as *mut _),
                std::mem::size_of::<TOKEN_LINKED_TOKEN>() as u32,
                &mut size,
            )
        };
        unsafe { CloseHandle(token) }.ok();
        got?;

        // Needs to be primary for CreateProcessWithTokenW; this is the step that
        // fails without SeTcbPrivilege.
        let mut primary = HANDLE::default();
        let dup = unsafe {
            DuplicateTokenEx(
                linked.LinkedToken,
                TOKEN_ACCESS_MASK(TOKEN_ALL_ACCESS),
                None,
                SecurityImpersonation,
                TokenPrimary,
                &mut primary,
            )
        };
        unsafe { CloseHandle(linked.LinkedToken) }.ok();
        dup?;

        let result = launch_with_token(primary, exe, cmdline);
        unsafe { CloseHandle(primary) }.ok();
        result
    }

    /// Strategy 4: shell token, but launched with CreateProcessAsUserW.
    ///
    /// This is the one that decides the architecture. `CreateProcessWithTokenW`
    /// silently ignores STARTUPINFOEX attribute lists, so a ConPTY pseudoconsole
    /// cannot be attached — which would force every normal tab through a
    /// separate medium-integrity helper process and a named-pipe protocol.
    /// `CreateProcessAsUserW` *does* honour extended startup info, so if it
    /// accepts the shell token, de-elevated ConPTYs can be created in-process
    /// and the helper is unnecessary.
    pub fn spawn_via_shell_token_as_user(exe: &Path, cmdline: &str) -> windows::core::Result<()> {
        let primary = shell_primary_token()?;

        let si = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut pi = PROCESS_INFORMATION::default();

        let app = wide(&exe.to_string_lossy());
        let mut cmd = wide(cmdline);

        let spawned = unsafe {
            CreateProcessAsUserW(
                Some(primary),
                PCWSTR(app.as_ptr()),
                Some(PWSTR(cmd.as_mut_ptr())),
                None,
                None,
                false,
                PROCESS_CREATION_FLAGS(CREATE_NO_WINDOW),
                None,
                PCWSTR::null(),
                &si,
                &mut pi,
            )
        };

        if spawned.is_ok() {
            unsafe {
                WaitForSingleObject(pi.hProcess, INFINITE);
                CloseHandle(pi.hProcess).ok();
                CloseHandle(pi.hThread).ok();
            }
        }
        unsafe { CloseHandle(primary) }.ok();
        spawned
    }

    /// Duplicate the desktop shell's token into a primary token we can launch with.
    fn shell_primary_token() -> windows::core::Result<HANDLE> {
        // GetShellWindow is the documented way to find the desktop shell without
        // guessing at process names.
        let hwnd = unsafe { GetShellWindow() };
        if hwnd.0.is_null() {
            return Err(windows::core::Error::from_thread());
        }

        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid == 0 {
            return Err(windows::core::Error::from_thread());
        }

        let shell = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION, false, pid) }?;

        let mut shell_token = HANDLE::default();
        let opened =
            unsafe { OpenProcessToken(shell, TOKEN_DUPLICATE | TOKEN_QUERY, &mut shell_token) };
        unsafe { CloseHandle(shell) }.ok();
        opened?;

        let mut primary = HANDLE::default();
        let dup = unsafe {
            DuplicateTokenEx(
                shell_token,
                TOKEN_ACCESS_MASK(TOKEN_ALL_ACCESS),
                None,
                SecurityImpersonation,
                TokenPrimary,
                &mut primary,
            )
        };
        unsafe { CloseHandle(shell_token) }.ok();
        dup?;

        Ok(primary)
    }

    /// Strategy 2: borrow the shell's (explorer.exe) medium-integrity token.
    pub fn spawn_via_shell_token(exe: &Path, cmdline: &str) -> windows::core::Result<()> {
        let primary = shell_primary_token()?;
        let result = launch_with_token(primary, exe, cmdline);
        unsafe { CloseHandle(primary) }.ok();
        result
    }

    /// Strategy 3: a SAFER "normal user" token, stamped to medium integrity.
    pub fn spawn_via_safer(exe: &Path, cmdline: &str) -> windows::core::Result<()> {
        let mut level = SAFER_LEVEL_HANDLE::default();
        unsafe {
            SaferCreateLevel(
                SAFER_SCOPEID_USER,
                SAFER_LEVELID_NORMALUSER,
                SAFER_LEVEL_OPEN,
                &mut level,
                None,
            )
        }?;

        let mut restricted = HANDLE::default();
        let computed = unsafe {
            SaferComputeTokenFromLevel(
                level,
                None,
                &mut restricted,
                SAFER_COMPUTE_TOKEN_FROM_LEVEL_FLAGS(0),
                None,
            )
        };
        unsafe { SaferCloseLevel(level) }.ok();
        computed?;

        // SAFER strips privileges but leaves integrity at High, so the child
        // would still be "elevated" in every way that matters. Stamp it down.
        let sid_text = wide(MEDIUM_INTEGRITY_SID);
        let mut sid = PSID::default();
        unsafe { ConvertStringSidToSidW(PCWSTR(sid_text.as_ptr()), &mut sid) }?;

        let label = TOKEN_MANDATORY_LABEL {
            Label: SID_AND_ATTRIBUTES {
                Sid: sid,
                Attributes: SE_GROUP_INTEGRITY,
            },
        };

        unsafe {
            SetTokenInformation(
                restricted,
                TokenIntegrityLevel,
                &label as *const _ as *const _,
                std::mem::size_of::<TOKEN_MANDATORY_LABEL>() as u32,
            )
        }?;

        // CreateProcessAsUserW (not ...WithToken) because a SAFER token derived
        // from our own does not require SeAssignPrimaryTokenPrivilege.
        let si = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut pi = PROCESS_INFORMATION::default();

        let app = wide(&exe.to_string_lossy());
        let mut cmd = wide(cmdline);

        let spawned = unsafe {
            CreateProcessAsUserW(
                Some(restricted),
                PCWSTR(app.as_ptr()),
                Some(PWSTR(cmd.as_mut_ptr())),
                None,
                None,
                false,
                PROCESS_CREATION_FLAGS(CREATE_NO_WINDOW),
                None,
                PCWSTR::null(),
                &si,
                &mut pi,
            )
        };

        if spawned.is_ok() {
            unsafe {
                WaitForSingleObject(pi.hProcess, INFINITE);
                CloseHandle(pi.hProcess).ok();
                CloseHandle(pi.hThread).ok();
            }
        }
        unsafe { CloseHandle(restricted) }.ok();
        spawned
    }
}
