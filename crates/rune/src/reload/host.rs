//! # Host Signaler
//!
//! Signals the host process when a reload is needed.

use std::fs;
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
    #[allow(clippy::unwrap_used)]
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
    pub fn signal(&self) -> ReloadResult<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();

        let signal_file = self.hot_dir.join(format!("reload_{timestamp}.signal"));
        fs::write(&signal_file, "")?;

        // Clean up old signal files (keep only last 10)
        self.cleanup_old_signals(10)?;

        Ok(())
    }

    /// Clean up signal files older than the last N.
    fn cleanup_old_signals(&self, keep_last: usize) -> ReloadResult<()> {
        let mut signals: Vec<_> = fs::read_dir(&self.hot_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "signal"))
            .collect();

        if signals.len() <= keep_last {
            return Ok(());
        }

        signals.sort_by_key(|e| {
            e.path()
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.strip_prefix("reload_"))
                .and_then(|s| s.parse::<u64>().ok())
        });

        // Keep only the last `keep_last` signals
        let to_remove = signals.len() - keep_last;
        for signal in signals.into_iter().take(to_remove) {
            let _ = fs::remove_file(signal.path());
        }

        Ok(())
    }

    /// Clear all reload signals.
    ///
    /// # Errors
    /// Returns an error if clearing fails.
    pub fn clear(&self) -> ReloadResult<()> {
        for entry in fs::read_dir(&self.hot_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "signal") {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }

    /// Get the current dylib path.
    ///
    /// # Errors
    /// Returns an error if reading fails.
    #[allow(clippy::unwrap_used)]
    pub fn current_dylib(&self) -> ReloadResult<Option<PathBuf>> {
        let current_link = self.hot_dir.join(".current");
        if current_link.exists() {
            Ok(Some(fs::read_link(current_link)?))
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
        fs::write(&self.state_file, "")?;
        Ok(())
    }

    /// Clear the restart needed flag.
    ///
    /// # Errors
    /// Returns an error if clearing fails.
    pub fn clear_restart_needed(&self) -> ReloadResult<()> {
        if self.state_file.exists() {
            fs::remove_file(&self.state_file)?;
        }
        Ok(())
    }
}
