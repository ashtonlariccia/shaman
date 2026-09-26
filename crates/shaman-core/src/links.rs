//! Opening links clicked in a terminal.
//!
//! Anything a terminal shows is written by whatever program is running in it,
//! including a remote one Shaman has no reason to trust, and an OSC 8
//! hyperlink's target is not even visible on screen. So the check lives here
//! rather than in the webview: only `http` and `https` are ever handed to the
//! OS. `file:`, `ms-settings:`, `search-ms:` and the other registered schemes
//! can launch programs or open shares, which is not something a line of
//! terminal output should be able to do with one click.

/// Longest URL we will open. Far beyond anything real, and it keeps a program
/// from handing ShellExecute a multi-megabyte argument.
const MAX_LEN: usize = 2048;

/// Why a link was refused. Shown to nobody but the log; the UI only offers
/// links that already passed the same rule.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    TooLong,
    NotWeb,
    NoHost,
    ControlCharacter,
}

/// Accept a web URL, or say why not.
pub fn check(url: &str) -> Result<(), Refusal> {
    if url.len() > MAX_LEN {
        return Err(Refusal::TooLong);
    }
    // Whitespace and control characters never belong in a URL, and a space or
    // quote is how an argument would break out of one on a command line.
    if url
        .chars()
        .any(|c| c.is_control() || c.is_whitespace() || c == '"')
    {
        return Err(Refusal::ControlCharacter);
    }
    let lower = url.to_ascii_lowercase();
    let rest = ["https://", "http://"]
        .iter()
        .find_map(|scheme| lower.strip_prefix(scheme))
        .ok_or(Refusal::NotWeb)?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() {
        return Err(Refusal::NoHost);
    }
    Ok(())
}

/// Open a checked URL in the user's default browser.
pub fn open(url: &str) -> Result<(), String> {
    check(url).map_err(|why| format!("refused to open link ({why:?})"))?;
    imp::shell_open(url)
}

#[cfg(windows)]
mod imp {
    // shell32 is linked by default under MSVC. Declared by hand, as elsewhere in
    // this crate, rather than growing the `windows` feature list for one call.
    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            hwnd: *mut core::ffi::c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show_cmd: i32,
        ) -> isize;
    }

    const SW_SHOWNORMAL: i32 = 1;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    pub fn shell_open(url: &str) -> Result<(), String> {
        let operation = wide("open");
        let file = wide(url);
        // SAFETY: both strings are NUL-terminated and outlive the call; the
        // null pointers are the documented "none" for the optional arguments.
        let code = unsafe {
            ShellExecuteW(
                core::ptr::null_mut(),
                operation.as_ptr(),
                file.as_ptr(),
                core::ptr::null(),
                core::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        // Documented: anything above 32 is success, anything else an error code.
        if code > 32 {
            Ok(())
        } else {
            Err(format!(
                "the browser could not be started (ShellExecute {code})"
            ))
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn shell_open(_url: &str) -> Result<(), String> {
        Err("opening links is only implemented on Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_urls_are_accepted() {
        for url in [
            "https://example.com",
            "http://example.com/path?q=1#frag",
            "HTTPS://Example.com/",
            "https://[::1]:8080/",
            "http://localhost:5173",
        ] {
            assert_eq!(check(url), Ok(()), "{url}");
        }
    }

    #[test]
    fn other_schemes_are_refused() {
        for url in [
            "file:///C:/Windows/System32/calc.exe",
            "ms-settings:privacy",
            "search-ms:query=x",
            "javascript:alert(1)",
            "\\\\server\\share",
            "example.com",
        ] {
            assert_eq!(check(url), Err(Refusal::NotWeb), "{url}");
        }
    }

    #[test]
    fn a_url_without_a_host_is_refused() {
        assert_eq!(check("https://"), Err(Refusal::NoHost));
        assert_eq!(check("http:///path"), Err(Refusal::NoHost));
    }

    #[test]
    fn whitespace_and_quotes_cannot_smuggle_arguments() {
        assert_eq!(check("https://a.com/ -x"), Err(Refusal::ControlCharacter));
        assert_eq!(check("https://a.com/\"x"), Err(Refusal::ControlCharacter));
        assert_eq!(check("https://a.com/\nx"), Err(Refusal::ControlCharacter));
    }

    #[test]
    fn absurd_lengths_are_refused() {
        let url = format!("https://a.com/{}", "x".repeat(MAX_LEN));
        assert_eq!(check(&url), Err(Refusal::TooLong));
    }
}
