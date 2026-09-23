//! Secret storage, and where Shaman keeps its files.
//!
//! Secrets are encrypted with **DPAPI** (`CryptProtectData`), which ties the
//! ciphertext to the current Windows user account. No master password to manage,
//! and a stolen `connections.json` is useless on another machine or to another
//! user on this one.
//!
//! Worth being clear about the limit: DPAPI protects against *other users and
//! copied files*, not against malware already running as you. Anything running
//! under your account can ask DPAPI to decrypt these too. A master-password mode
//! (Argon2id + AES-GCM) is the upgrade path if that ever matters.

use std::path::PathBuf;

use crate::{Error, Result};

/// `%APPDATA%\shaman`, created if missing.
///
/// `SHAMAN_DATA_DIR` overrides it. That exists so tests never touch the real
/// known-hosts file or credential store — a test that trusts `127.0.0.1` should
/// not leave an entry in the user's own trust list.
pub fn data_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("SHAMAN_DATA_DIR") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir)?;
        return Ok(dir);
    }

    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| Error::Other(anyhow::anyhow!("APPDATA is not set")))?;

    let dir = base.join("shaman");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Hex rather than base64 to avoid a dependency for a handful of small blobs.
pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex(text: &str) -> Result<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return Err(Error::Other(anyhow::anyhow!(
            "stored secret is malformed (odd length)"
        )));
    }
    (0..text.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&text[i..i + 2], 16)
                .map_err(|e| Error::Other(anyhow::anyhow!("stored secret is malformed: {e}")))
        })
        .collect()
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::LocalFree;
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
    };

    use crate::{Error, Result};

    /// Do not show a UI prompt; this must never block a background call.
    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        }
    }

    /// Copy the result out and hand the OS buffer back.
    ///
    /// # Safety
    /// `out` must be a blob returned by DPAPI and not yet freed.
    unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        let slice = std::slice::from_raw_parts(out.pbData, out.cbData as usize);
        let owned = slice.to_vec();
        // DPAPI allocates with LocalAlloc; skipping this leaks on every call.
        let _ = LocalFree(Some(windows::Win32::Foundation::HLOCAL(out.pbData as _)));
        owned
    }

    pub fn protect(secret: &[u8]) -> Result<Vec<u8>> {
        let input = blob(secret);
        let mut output = CRYPT_INTEGER_BLOB::default();

        unsafe {
            CryptProtectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
            .map_err(|e| Error::Other(anyhow::anyhow!("could not encrypt the secret: {e}")))?;

            Ok(take(output))
        }
    }

    pub fn unprotect(blob_in: &[u8]) -> Result<Vec<u8>> {
        let input = blob(blob_in);
        let mut output = CRYPT_INTEGER_BLOB::default();

        unsafe {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
            .map_err(|e| {
                Error::Other(anyhow::anyhow!(
                    "could not decrypt the stored secret — it may belong to a different \
                     Windows account or machine: {e}"
                ))
            })?;

            Ok(take(output))
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use crate::{Error, Result};

    pub fn protect(_secret: &[u8]) -> Result<Vec<u8>> {
        Err(Error::Other(anyhow::anyhow!("DPAPI is Windows-only")))
    }
    pub fn unprotect(_blob: &[u8]) -> Result<Vec<u8>> {
        Err(Error::Other(anyhow::anyhow!("DPAPI is Windows-only")))
    }
}

pub use imp::{protect, unprotect};

/// Encrypt a secret and return it as hex, ready to sit in JSON.
pub fn seal(secret: &str) -> Result<String> {
    Ok(to_hex(&protect(secret.as_bytes())?))
}

/// Reverse of [`seal`].
pub fn unseal(sealed: &str) -> Result<String> {
    let plain = unprotect(&from_hex(sealed)?)?;
    String::from_utf8(plain)
        .map_err(|e| Error::Other(anyhow::anyhow!("stored secret is not valid UTF-8: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let bytes = [0x00u8, 0x0f, 0x7f, 0x80, 0xff];
        assert_eq!(to_hex(&bytes), "000f7f80ff");
        assert_eq!(from_hex("000f7f80ff").unwrap(), bytes);
    }

    #[test]
    fn malformed_hex_is_rejected_rather_than_panicking() {
        assert!(from_hex("abc").is_err(), "odd length");
        assert!(from_hex("zz").is_err(), "not hex");
    }

    #[test]
    #[cfg(windows)]
    fn a_sealed_secret_round_trips() {
        let sealed = seal("hunter2").expect("seal");
        assert!(!sealed.contains("hunter2"), "must not store plaintext");
        assert_eq!(unseal(&sealed).expect("unseal"), "hunter2");
    }

    #[test]
    #[cfg(windows)]
    fn tampered_ciphertext_fails_instead_of_returning_garbage() {
        let mut sealed = seal("hunter2").expect("seal");
        // Flip a byte in the middle of the blob.
        let middle = sealed.len() / 2;
        let flipped = if &sealed[middle..middle + 1] == "a" { "b" } else { "a" };
        sealed.replace_range(middle..middle + 1, flipped);

        assert!(unseal(&sealed).is_err(), "DPAPI must reject tampering");
    }
}
