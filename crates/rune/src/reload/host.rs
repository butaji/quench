//! # Host Signaler
//!
//! Signals the host process when a reload is needed.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use super::ReloadResult;

/// Signals the host process about reload state.
pub struct HostSignaler {
    /// Hot directory path
    hot_dir: PathBuf,
    /// State file for protocol changes
    state_file: PathBuf,
}

impl HostSignaler {
    /// Create a new host signaler.
    ///
    /// # Errors
    /// Returns an error if initialization fails.
    pub fn new(hot_dir: &Path) -> ReloadResult<Self> {
        let hot_dir = hot_dir.to_path_buf();
        let state_file = hot_dir.join("restart_needed");
        Ok(Self {
            hot_dir,
            state_file,
        })
    }

    /// Signal that a reload is needed.
    ///
    /// # Errors
    /// Returns an error if signaling fails.
    #[allow(clippy::unwrap_used)]
    pub fn signal(&self) -> ReloadResult<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();

        let signal_file = self.hot_dir.join(format!("reload_{timestamp}.signal"));
        std::fs::write(&signal_file, "")?;
        Ok(())
    }

    /// Clear all reload signals.
    ///
    /// # Errors
    /// Returns an error if clearing fails.
    pub fn clear(&self) -> ReloadResult<()> {
        for entry in std::fs::read_dir(&self.hot_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "signal") {
                std::fs::remove_file(path)?;
            }
        }
        Ok(())
    }

    /// Get the current dylib path.
    ///
    /// # Errors
    /// Returns an error if reading fails.
    pub fn current_dylib(&self) -> ReloadResult<Option<PathBuf>> {
        let current_link = self.hot_dir.join(".current");
        if current_link.exists() {
            Ok(Some(std::fs::read_link(current_link)?))
        } else {
            Ok(None)
        }
    }

    /// Check if a restart is needed (protocol changed).
    ///
    /// # Errors
    /// Returns an error if checking fails.
    pub fn should_restart(&self) -> ReloadResult<bool> {
        Ok(self.state_file.exists())
    }

    /// Mark that a restart is needed.
    ///
    /// # Errors
    /// Returns an error if marking fails.
    pub fn mark_restart_needed(&self) -> ReloadResult<()> {
        std::fs::write(&self.state_file, "")?;
        Ok(())
    }

    /// Clear the restart needed flag.
    ///
    /// # Errors
    /// Returns an error if clearing fails.
    pub fn clear_restart_needed(&self) -> ReloadResult<()> {
        if self.state_file.exists() {
            std::fs::remove_file(&self.state_file)?;
        }
        Ok(())
    }
}
