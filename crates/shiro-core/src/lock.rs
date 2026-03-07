//! Single-writer file lock for shiro.
//!
//! Write operations acquire an exclusive lock via a PID file.
//! Read operations do not require a lock.

use std::fs;
use std::io::Write;

use camino::{Utf8Path, Utf8PathBuf};

use crate::error::ShiroError;

/// An acquired write lock. Automatically released on drop.
#[derive(Debug)]
pub struct WriteLock {
    path: Utf8PathBuf,
}

impl WriteLock {
    /// Attempt to acquire the write lock.
    ///
    /// Returns `Err(ShiroError::LockBusy)` if another process holds the lock.
    pub fn acquire(lock_dir: &Utf8Path) -> Result<Self, ShiroError> {
        let path = lock_dir.join("write.lock");

        // Ensure lock directory exists.
        fs::create_dir_all(lock_dir.as_std_path())?;

        // Check for stale lock.
        if path.as_std_path().exists() {
            let contents = fs::read_to_string(path.as_std_path()).unwrap_or_default();
            if let Ok(pid) = contents.trim().parse::<u32>() {
                if is_process_alive(pid) {
                    return Err(ShiroError::LockBusy {
                        message: format!("write lock held by PID {pid}"),
                    });
                }
                // Stale lock — process is dead.
                eprintln!("shiro: removing stale write lock (PID {pid})");
            }
            // Remove invalid/stale lock file.
            let _ = fs::remove_file(path.as_std_path());
        }

        // Write our PID.
        let mut file = fs::File::create(path.as_std_path())?;
        write!(file, "{}", std::process::id())?;
        file.sync_all()?;

        Ok(Self { path })
    }

    /// Release the lock explicitly (also happens on drop).
    pub fn release(self) {
        // Drop does the work.
    }
}

impl Drop for WriteLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.path.as_std_path());
    }
}

/// Check whether a process with the given PID is still running.
#[cfg(unix)]
fn is_process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_process_alive(_pid: u32) -> bool {
    // Conservative: assume alive on non-Unix.
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_and_release() {
        let dir = tempfile::tempdir().unwrap();
        let lock_dir = Utf8Path::from_path(dir.path()).unwrap();
        let lock = WriteLock::acquire(lock_dir).unwrap();
        assert!(lock_dir.join("write.lock").as_std_path().exists());
        drop(lock);
        assert!(!lock_dir.join("write.lock").as_std_path().exists());
    }

    #[test]
    fn double_acquire_fails() {
        let dir = tempfile::tempdir().unwrap();
        let lock_dir = Utf8Path::from_path(dir.path()).unwrap();
        let _lock = WriteLock::acquire(lock_dir).unwrap();
        let err = WriteLock::acquire(lock_dir).unwrap_err();
        assert!(err.to_string().contains("lock"), "got: {err}");
    }

    #[test]
    fn stale_lock_removed() {
        let dir = tempfile::tempdir().unwrap();
        let lock_dir = Utf8Path::from_path(dir.path()).unwrap();
        let lock_path = lock_dir.join("write.lock");
        // Write a PID that doesn't exist.
        std::fs::write(lock_path.as_std_path(), "999999999").unwrap();
        // Should succeed because that PID is dead.
        let lock = WriteLock::acquire(lock_dir).unwrap();
        drop(lock);
    }
}
