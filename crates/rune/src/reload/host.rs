//! # Host Signaler
//!
//! Signals the host binary to reload the dylib.

use std::path::{Path, PathBuf};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use super::ReloadResult;

/// Signals the host to reload.
pub struct HostSignaler {
    /// Signal file path
    signal_path: PathBuf,
    /// Hot directory
    hot_dir: PathBuf,
}

impl HostSignaler {
    /// Create a new signaler.
    pub fn new(hot_dir: &Path) -> ReloadResult<Self> {
        let signal_path = hot_dir.join(".reload-signal");
        Ok(Self {
            signal_path,
            hot_dir: hot_dir.to_path_buf(),
        })
    }

    /// Signal the host to reload.
    pub fn signal(&self) -> ReloadResult<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        fs::write(&self.signal_path, timestamp.to_string())?;
        Ok(())
    }

    /// Clear the signal.
    pub fn clear(&self) -> ReloadResult<()> {
        if self.signal_path.exists() {
            fs::remove_file(&self.signal_path)?;
        }
        Ok(())
    }

    /// Get the path to the current dylib.
    pub fn current_dylib(&self) -> ReloadResult<Option<PathBuf>> {
        let current = self.hot_dir.join(".current");

        if !current.exists() {
            return Ok(None);
        }

        let target = fs::read_link(&current)?;
        Ok(Some(target))
    }

    /// Check if host should restart (protocol changed).
    pub fn should_restart(&self) -> ReloadResult<bool> {
        let restart_file = self.hot_dir.join(".restart-needed");
        Ok(restart_file.exists())
    }

    /// Mark that a restart is needed.
    pub fn mark_restart_needed(&self) -> ReloadResult<()> {
        let restart_file = self.hot_dir.join(".restart-needed");
        fs::write(&restart_file, "1")?;
        Ok(())
    }

    /// Clear the restart marker.
    pub fn clear_restart_needed(&self) -> ReloadResult<()> {
        let restart_file = self.hot_dir.join(".restart-needed");
        if restart_file.exists() {
            fs::remove_file(&restart_file)?;
        }
        Ok(())
    }
}
