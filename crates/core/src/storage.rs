//! Bounded, current-user Windows DPAPI storage. No plaintext fallback.
use crate::model::{AppError, ErrorCode};
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

const MAGIC: &[u8] = b"ECHOCORD-TOKEN\0";
#[cfg(any(windows, test))]
const VERSION: u8 = 1;
#[cfg(any(windows, test))]
const HEADER_LEN: usize = MAGIC.len() + 1 + 4;
const MAX_TOKEN_BYTES: usize = 4096;
#[cfg(any(windows, test))]
const MAX_FILE_BYTES: usize = 16 * 1024;

fn error(message: &'static str) -> AppError {
    AppError::new(ErrorCode::Configuration, message)
}

/// Resolve the platform-local data directory without creating it.
/// Callers can bypass this default by passing their own paths to save/load.
pub fn default_data_dir() -> Result<PathBuf, AppError> {
    ProjectDirs::from("", "", "Echocord")
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .ok_or_else(|| error("The application data directory is unavailable."))
}

/// Recognize the encrypted file family, including truncated or unknown versions.
/// This is a format discriminator, not a validation or decryption check.
pub fn is_encrypted(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

fn validate_token(token: &str) -> Result<(), AppError> {
    if token.is_empty()
        || token.len() > MAX_TOKEN_BYTES
        || token.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(error(
            "The bot token must be nonempty, at most 4096 bytes, and contain no whitespace or control characters.",
        ));
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn decode_file(bytes: &[u8]) -> Result<&[u8], AppError> {
    if bytes.len() < HEADER_LEN || bytes.len() > MAX_FILE_BYTES || !is_encrypted(bytes) {
        return Err(error(
            "The encrypted bot token file is invalid or too large.",
        ));
    }
    if bytes[MAGIC.len()] != VERSION {
        return Err(error(
            "The encrypted bot token file version is unsupported.",
        ));
    }
    let size = u32::from_le_bytes(
        bytes[MAGIC.len() + 1..HEADER_LEN]
            .try_into()
            .expect("fixed header length"),
    ) as usize;
    if size == 0 || size != bytes.len() - HEADER_LEN {
        return Err(error(
            "The encrypted bot token file is invalid or too large.",
        ));
    }
    Ok(&bytes[HEADER_LEN..])
}

/// Encrypt before touching disk, then atomically replace the caller's file.
/// An absent parent directory is created only on supported platforms.
pub fn save_token(path: &Path, token: &str) -> Result<(), AppError> {
    validate_token(token)?;
    #[cfg(windows)]
    {
        let encrypted = windows::protect(token.as_bytes())?;
        if encrypted.is_empty() || encrypted.len() > MAX_FILE_BYTES - HEADER_LEN {
            return Err(error("The encrypted bot token is too large to store."));
        }
        let mut bytes = Vec::with_capacity(HEADER_LEN + encrypted.len());
        bytes.extend_from_slice(MAGIC);
        bytes.push(VERSION);
        bytes.extend_from_slice(&(encrypted.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&encrypted);
        windows::atomic_write(path, &bytes)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(error(
            "Encrypted bot token storage is supported only on Windows.",
        ))
    }
}

/// Load only a bounded versioned encrypted file; never interpret plaintext.
pub fn load_token(path: &Path) -> Result<String, AppError> {
    #[cfg(windows)]
    {
        use std::{fs::File, io::Read};
        let file = File::open(path)
            .map_err(|_| error("The encrypted bot token file could not be opened."))?;
        if !file
            .metadata()
            .map_err(|_| error("The encrypted bot token file could not be inspected."))?
            .is_file()
        {
            return Err(error("The encrypted bot token file is not a regular file."));
        }
        let mut bytes = Vec::new();
        file.take((MAX_FILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| error("The encrypted bot token file could not be read."))?;
        let token = windows::unprotect(decode_file(&bytes)?)?;
        validate_token(&token)?;
        Ok(token)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(error(
            "Encrypted bot token storage is supported only on Windows.",
        ))
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        fs::{self, OpenOptions},
        io::Write,
        os::windows::ffi::OsStrExt,
        ptr::{null, null_mut},
        sync::atomic::{AtomicU64, Ordering},
    };
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
        Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW},
    };

    // Application/version binding, not a secret or a substitute for DPAPI.
    const ENTROPY: &[u8] = b"Echocord bot token storage v1";

    struct LocalBlob(CRYPT_INTEGER_BLOB);

    impl Drop for LocalBlob {
        fn drop(&mut self) {
            if !self.0.pbData.is_null() {
                // SAFETY: DPAPI owns this allocation until LocalFree. Volatile writes
                // clear its sensitive output before releasing the allocation.
                unsafe {
                    for i in 0..self.0.cbData as usize {
                        self.0.pbData.add(i).write_volatile(0);
                    }
                    LocalFree(self.0.pbData.cast());
                }
            }
        }
    }

    fn crypt(input: &[u8], decrypt: bool) -> Result<LocalBlob, AppError> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: input.len() as u32,
            pbData: input.as_ptr().cast_mut(),
        };
        let entropy = CRYPT_INTEGER_BLOB {
            cbData: ENTROPY.len() as u32,
            pbData: ENTROPY.as_ptr().cast_mut(),
        };
        let mut output = LocalBlob(CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: null_mut(),
        });
        // SAFETY: bounded input/entropy live through this call; optional pointers
        // are null, and LocalBlob releases the returned allocation. No machine
        // scope flag is set. Both operations explicitly prohibit any UI.
        let succeeded = unsafe {
            if decrypt {
                CryptUnprotectData(
                    &input,
                    null_mut(),
                    &entropy,
                    null(),
                    null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output.0,
                )
            } else {
                CryptProtectData(
                    &input,
                    null(),
                    &entropy,
                    null(),
                    null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output.0,
                )
            }
        };
        if succeeded == 0 {
            return Err(error(if decrypt {
                "The bot token could not be decrypted for the current Windows user. Save it again in setup."
            } else {
                "The bot token could not be encrypted for the current Windows user."
            }));
        }
        let limit = if decrypt {
            MAX_TOKEN_BYTES
        } else {
            MAX_FILE_BYTES - HEADER_LEN
        };
        if output.0.pbData.is_null() || output.0.cbData == 0 || output.0.cbData as usize > limit {
            return Err(error(
                "The encrypted bot token data is invalid or too large.",
            ));
        }
        Ok(output)
    }

    pub(super) fn protect(input: &[u8]) -> Result<Vec<u8>, AppError> {
        let output = crypt(input, false)?;
        // SAFETY: successful DPAPI output is nonnull and bounded above.
        Ok(
            unsafe { std::slice::from_raw_parts(output.0.pbData, output.0.cbData as usize) }
                .to_vec(),
        )
    }

    pub(super) fn unprotect(input: &[u8]) -> Result<String, AppError> {
        let output = crypt(input, true)?;
        // SAFETY: successful DPAPI output is nonnull and bounded above. Decode
        // before copying so invalid plaintext never escapes in an UTF-8 error.
        let bytes =
            unsafe { std::slice::from_raw_parts(output.0.pbData, output.0.cbData as usize) };
        let token =
            std::str::from_utf8(bytes).map_err(|_| error("The decrypted bot token is invalid."))?;
        validate_token(token)?;
        Ok(token.to_owned())
    }

    struct PendingFile(PathBuf);

    impl Drop for PendingFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn wide_path(path: &Path) -> Result<Vec<u16>, AppError> {
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        if wide.contains(&0) {
            return Err(error("The bot token file destination is invalid."));
        }
        wide.push(0);
        Ok(wide)
    }

    pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
        static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
        if path.file_name().is_none() {
            return Err(error("The bot token file destination is invalid."));
        }
        let destination = wide_path(path)?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|_| error("The bot token storage directory could not be created."))?;
        for _ in 0..32 {
            let temporary = parent.join(format!(
                ".echocord-token-{}-{}.tmp",
                std::process::id(),
                NEXT_FILE.fetch_add(1, Ordering::Relaxed)
            ));
            let mut file = match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => {
                    return Err(error(
                        "The encrypted bot token temporary file could not be created.",
                    ));
                }
            };
            let pending = PendingFile(temporary);
            let written = file.write_all(bytes).and_then(|()| file.sync_all());
            drop(file);
            written.map_err(|_| error("The encrypted bot token file could not be written."))?;
            let source = wide_path(&pending.0)?;
            // SAFETY: both buffers are NUL-terminated and live for the call.
            // The sibling source stays on the same filesystem: no copy fallback.
            if unsafe {
                MoveFileExW(
                    source.as_ptr(),
                    destination.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            } == 0
            {
                return Err(error("The encrypted bot token file could not be replaced."));
            }
            return Ok(());
        }
        Err(error(
            "The encrypted bot token temporary file is unavailable. Try saving again.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(payload: &[u8]) -> Vec<u8> {
        let mut bytes = MAGIC.to_vec();
        bytes.push(VERSION);
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn rejects_malformed_and_oversize_envelopes() {
        let valid = envelope(&[1, 2, 3]);
        assert!(decode_file(&valid).is_ok());
        for size in 0..valid.len() {
            assert!(decode_file(&valid[..size]).is_err());
        }
        assert!(decode_file(b"synthetic-plaintext").is_err());
        assert!(decode_file(&envelope(&[])).is_err());
        assert!(decode_file(&envelope(&vec![0; MAX_FILE_BYTES])).is_err());
        let mut unknown = valid.clone();
        unknown[MAGIC.len()] = VERSION + 1;
        assert!(is_encrypted(&unknown));
        assert!(decode_file(&unknown).is_err());
        let mut huge_length = valid.clone();
        huge_length[MAGIC.len() + 1..HEADER_LEN].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_file(&huge_length).is_err());
        let mut trailing = valid;
        trailing.push(0);
        assert!(decode_file(&trailing).is_err());
        assert!(is_encrypted(MAGIC));
        assert!(!is_encrypted(b"synthetic-plaintext"));
    }

    #[test]
    fn token_validation_is_bounded_and_errors_are_redacted() {
        assert!(validate_token("synthetic-token").is_ok());
        assert!(validate_token(&"a".repeat(MAX_TOKEN_BYTES)).is_ok());
        for token in [
            "",
            "synthetic token",
            "synthetic\n",
            "synthetic\0",
            "synthetic\u{2003}",
            &"a".repeat(MAX_TOKEN_BYTES + 1),
        ] {
            let failure = validate_token(token).unwrap_err();
            assert!(!format!("{failure:?}").contains("synthetic"));
            assert!(!failure.to_string().contains("synthetic"));
        }
    }

    #[test]
    fn default_directory_matches_platform_local_project_dirs() {
        let expected = ProjectDirs::from("", "", "Echocord").unwrap();
        assert!(default_data_dir().unwrap() == expected.data_local_dir());
    }

    #[cfg(windows)]
    #[test]
    fn rejects_dpapi_blob_from_another_entropy_domain() {
        use std::ptr::{null, null_mut};
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::Cryptography::{
                CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
            },
        };
        let token = b"synthetic-unrelated-token";
        let input = CRYPT_INTEGER_BLOB {
            cbData: token.len() as u32,
            pbData: token.as_ptr().cast_mut(),
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: null_mut(),
        };
        // SAFETY: valid synthetic input and output live through the call. The
        // native allocation is copied and freed before the assertion below.
        let succeeded = unsafe {
            CryptProtectData(
                &input,
                null(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        assert!(succeeded != 0 && !output.pbData.is_null());
        let encrypted = unsafe {
            let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
            LocalFree(output.pbData.cast());
            bytes
        };
        let Err(failure) = windows::unprotect(&encrypted) else {
            panic!("a DPAPI blob from another entropy domain was accepted");
        };
        assert!(!format!("{failure:?}").contains("synthetic"));
    }

    #[cfg(not(windows))]
    #[test]
    fn unsupported_operations_do_not_access_files() {
        let path = Path::new("artifacts/storage-probe-synthetic-not-created/token.bin");
        let saved = save_token(path, "synthetic-token").unwrap_err();
        let loaded = load_token(path).unwrap_err();
        assert!(saved.message.contains("only on Windows"));
        assert!(loaded.message.contains("only on Windows"));
        assert!(!path.exists());
        assert!(!format!("{saved:?} {loaded:?}").contains("storage-probe-synthetic"));
    }
}
