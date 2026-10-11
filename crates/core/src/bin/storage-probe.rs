//! Synthetic secure-storage regression. Requires an explicit absolute directory
//! for a uniquely named disposable test child; never reads real credentials or writes application defaults.
use echocord_core::model;
#[path = "../storage.rs"]
mod storage;

#[cfg(windows)]
fn run() -> Result<(), model::AppError> {
    use model::{AppError, ErrorCode};
    use std::{fs, os::windows::fs::OpenOptionsExt, path::PathBuf};

    fn check(condition: bool, message: &'static str) -> Result<(), AppError> {
        if condition {
            Ok(())
        } else {
            Err(AppError::new(ErrorCode::Internal, message))
        }
    }
    fn rejected(path: &std::path::Path) -> Result<(), AppError> {
        let failure = storage::load_token(path).err().ok_or_else(|| {
            AppError::new(ErrorCode::Internal, "Invalid encrypted data was accepted.")
        })?;
        let rendered = format!("{failure:?} {failure}");
        check(
            !rendered.contains("synthetic") && !rendered.contains(path.to_string_lossy().as_ref()),
            "A storage failure exposed private input.",
        )
    }
    fn io_error(_: std::io::Error) -> AppError {
        AppError::new(ErrorCode::Internal, "Synthetic storage probe I/O failed.")
    }

    let args: Vec<_> = std::env::args_os().skip(1).collect();
    check(
        args.len() == 1,
        "Pass one explicit absolute directory for disposable test data.",
    )?;
    let root = PathBuf::from(&args[0]);
    check(
        root.is_absolute(),
        "The synthetic destination must be an absolute directory.",
    )?;
    check(
        root.is_dir(),
        "Create the explicit synthetic directory before running the probe.",
    )?;
    // Reading the default's location is safe; no application profile is opened.
    let _ = storage::default_data_dir()?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| AppError::new(ErrorCode::Internal, "Probe clock is unavailable."))?
        .as_nanos();
    let directory = root.join(format!(
        "storage-probe-synthetic-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&directory).map_err(io_error)?;
    let path = directory.join("nested/token.bin");
    let first = "synthetic-storage-token-first";
    let second = "synthetic-storage-token-second";

    storage::save_token(&path, first)?;
    check(
        storage::load_token(&path)? == first,
        "The Windows synthetic roundtrip failed.",
    )?;
    let original = fs::read(&path).map_err(io_error)?;
    check(
        storage::is_encrypted(&original),
        "The saved file has no encrypted envelope.",
    )?;
    check(
        !original
            .windows(first.len())
            .any(|part| part == first.as_bytes()),
        "The saved file contains plaintext.",
    )?;
    storage::save_token(&path, second)?;
    check(
        storage::load_token(&path)? == second,
        "Replacing a saved token failed.",
    )?;
    let replacement = fs::read(&path).map_err(io_error)?;
    check(
        !replacement
            .windows(second.len())
            .any(|part| part == second.as_bytes()),
        "The replacement contains plaintext.",
    )?;

    let maximum = "a".repeat(4096);
    storage::save_token(&path, &maximum)?;
    check(
        storage::load_token(&path)? == maximum,
        "The maximum supported token did not roundtrip.",
    )?;
    storage::save_token(&path, second)?;
    let replacement = fs::read(&path).map_err(io_error)?;

    // Deny delete sharing on the existing file to exercise a replacement
    // failure after the encrypted sibling file has been written and flushed.
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ)
        .open(&path)
        .map_err(io_error)?;
    check(
        storage::save_token(&path, first).is_err(),
        "A locked destination was replaced.",
    )?;
    drop(lock);
    check(
        fs::read(&path).map_err(io_error)? == replacement,
        "A failed replacement changed the committed token.",
    )?;

    for invalid in ["", "synthetic invalid", "synthetic\n", &"a".repeat(4097)] {
        check(
            storage::save_token(&path, invalid).is_err(),
            "An invalid token was accepted.",
        )?;
        check(
            fs::read(&path).map_err(io_error)? == replacement,
            "Validation failure changed the existing encrypted file.",
        )?;
    }

    let bad = directory.join("invalid.bin");
    for size in [0, 1, 14, 18, original.len() - 1] {
        fs::write(&bad, &original[..size]).map_err(io_error)?;
        rejected(&bad)?;
    }
    let mut unknown = original.clone();
    unknown[b"ECHOCORD-TOKEN\0".len()] = 2;
    fs::write(&bad, unknown).map_err(io_error)?;
    rejected(&bad)?;
    let mut invalid_length = original.clone();
    let length_start = b"ECHOCORD-TOKEN\0".len() + 1;
    invalid_length[length_start..length_start + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    fs::write(&bad, invalid_length).map_err(io_error)?;
    rejected(&bad)?;
    let mut trailing = original.clone();
    trailing.push(0);
    fs::write(&bad, trailing).map_err(io_error)?;
    rejected(&bad)?;
    let mut corrupt = original.clone();
    let last = corrupt.len() - 1;
    corrupt[last] ^= 0x80;
    fs::write(&bad, corrupt).map_err(io_error)?;
    rejected(&bad)?;
    // Valid envelope size, completely invalid DPAPI blob.
    let mut wrong_blob = original.clone();
    wrong_blob[b"ECHOCORD-TOKEN\0".len() + 1 + 4..].fill(0x5a);
    fs::write(&bad, wrong_blob).map_err(io_error)?;
    rejected(&bad)?;
    for bytes in [
        vec![0; 16 * 1024 + 1],
        b"synthetic-plaintext-token".to_vec(),
    ] {
        fs::write(&bad, bytes).map_err(io_error)?;
        rejected(&bad)?;
    }
    rejected(&directory.join("missing-synthetic-token.bin"))?;

    // A directory cannot be replaced with a file. The encrypted temp must be
    // cleaned, and the earlier committed credential must remain readable.
    let blocked = directory.join("blocked.bin");
    fs::create_dir(&blocked).map_err(io_error)?;
    check(
        storage::save_token(&blocked, first).is_err(),
        "An invalid destination was accepted.",
    )?;
    check(
        storage::load_token(&path)? == second,
        "A failed save damaged the committed token.",
    )?;
    for entry in fs::read_dir(&directory).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        check(
            !entry.file_name().to_string_lossy().ends_with(".tmp"),
            "A failed save left a temporary file.",
        )?;
    }
    for entry in fs::read_dir(path.parent().unwrap()).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        check(
            !entry.file_name().to_string_lossy().ends_with(".tmp"),
            "Replacing a token left a temporary file.",
        )?;
    }
    // Remove only this invocation's synthetic directory, never supplied roots.
    fs::remove_dir_all(&directory).map_err(io_error)?;
    println!("Synthetic Windows storage checks passed.");
    Ok(())
}

fn main() {
    #[cfg(windows)]
    if let Err(failure) = run() {
        eprintln!("{failure}");
        std::process::exit(1);
    }
    #[cfg(not(windows))]
    {
        // Exercise public APIs without filesystem access on unsupported hosts.
        let _ = storage::default_data_dir();
        let _ = storage::is_encrypted(&[]);
        let path = std::path::Path::new("artifacts/storage-probe-synthetic-not-created/token.bin");
        assert!(storage::save_token(path, "synthetic-token").is_err());
        assert!(storage::load_token(path).is_err());
        eprintln!("The synthetic native storage probe requires Windows.");
        std::process::exit(2);
    }
}
