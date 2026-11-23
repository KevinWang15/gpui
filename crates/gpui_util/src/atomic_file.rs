//! Crash-safe atomic file writing with Windows rename retry backoff.
//!
//! Writes bytes into a uniquely named temporary file in the destination directory, flushes with
//! `sync_all`, and atomically replaces the target path. On Unix, the temporary file is created
//! with `0o600` permissions so secrets (such as API tokens) are never exposed world-readable.
//! On Windows, `std::fs::rename` / replacement retries with exponential backoff when antivirus
//! scanners or Windows Search Indexer briefly hold a sharing lock (`ERROR_ACCESS_DENIED`,
//! `ERROR_SHARING_VIOLATION`, `ERROR_LOCK_VIOLATION`).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static TEMP_SEQ: AtomicU64 = AtomicU64::new(1);

/// Atomically writes `bytes` to `path`, creating parent directories if needed.
pub fn atomic_write(path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
    let path = path.as_ref();
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    if let Some(dir) = parent {
        fs::create_dir_all(dir)?;
    }

    let stage_dir = parent.unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("gpui-file");

    let (temp_path, mut file) = create_unique_temp(stage_dir, file_name)?;
    let write_res = (|| -> io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    })();
    drop(file);

    if let Err(err) = write_res {
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }

    if let Err(err) = persist_temp(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }

    Ok(())
}

/// Atomically writes UTF-8 `text` to `path`.
pub fn atomic_write_str(path: impl AsRef<Path>, text: &str) -> io::Result<()> {
    atomic_write(path, text.as_bytes())
}

fn create_unique_temp(dir: &Path, base_name: &str) -> io::Result<(PathBuf, File)> {
    let pid = std::process::id();
    for _ in 0..64 {
        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let candidate = dir.join(format!(".{base_name}.{pid}.{seq}.tmp"));
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        match opts.open(&candidate) {
            Ok(file) => return Ok((candidate, file)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate unique temporary file for atomic write",
    ))
}

#[cfg(target_os = "windows")]
fn is_retryable_windows_rename_error(err: &io::Error) -> bool {
    matches!(err.raw_os_error(), Some(5 | 32 | 33))
        || err.kind() == io::ErrorKind::PermissionDenied
}

#[cfg(target_os = "windows")]
fn persist_temp(temp_path: &Path, target: &Path) -> io::Result<()> {
    // Exponential backoff covering transient Windows Defender / Search Indexer sharing locks.
    const DELAYS_MS: [u64; 8] = [2, 4, 8, 16, 32, 64, 128, 250];
    let mut last_err = None;
    for attempt in 0..=DELAYS_MS.len() {
        match fs::rename(temp_path, target) {
            Ok(()) => return Ok(()),
            Err(err) if is_retryable_windows_rename_error(&err) && attempt < DELAYS_MS.len() => {
                last_err = Some(err);
                std::thread::sleep(Duration::from_millis(DELAYS_MS[attempt]));
            }
            Err(err) => return Err(err),
        }
    }
    Err(last_err.unwrap_or_else(|| io::Error::other("atomic rename failed")))
}

#[cfg(not(target_os = "windows"))]
fn persist_temp(temp_path: &Path, target: &Path) -> io::Result<()> {
    let _ = Duration::from_millis(0);
    fs::rename(temp_path, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_creates_and_replaces_file() {
        let dir = std::env::temp_dir().join(format!(
            "gpui-atomic-test-{}-{}",
            std::process::id(),
            TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let path = dir.join("nested").join("config.json");

        atomic_write_str(&path, r#"{"v":1}"#).expect("initial write");
        assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"v":1}"#);

        atomic_write_str(&path, r#"{"v":2}"#).expect("overwrite");
        assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"v":2}"#);

        // No stray temp files remain in the target directory.
        let entries: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(entries.len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }
}
