//! # Dylib Watcher
//!
//! Watches for changes in the hot reload directory and signals the host.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;
use notify::{Watcher, RecommendedWatcher, RecursiveMode, Event, EventKind};
use super::{ReloadResult, ReloadError};

/// Watches for dylib changes and notifies the host.
pub struct DylibWatcher {
    /// Watch directory
    watch_dir: PathBuf,
    /// Notify watcher
    #[allow(dead_code)]
    watcher: RecommendedWatcher,
    /// Event receiver
    receiver: mpsc::Receiver<ReloadEvent>,
    /// Current dylib path
    current: Option<PathBuf>,
}

impl DylibWatcher {
    /// Create a new watcher.
    ///
    /// # Errors
    /// Returns an error if the watcher cannot be created.
    pub fn new(hot_dir: &Path) -> ReloadResult<Self> {
        let watch_dir = hot_dir.to_path_buf();
        let (tx, rx) = mpsc::channel();

        let watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                    let _ = tx.send(ReloadEvent::Changed);
                }
            }
        })
        .map_err(|e| ReloadError::Library(e.to_string()))?;

        let mut watcher = watcher;
        watcher
            .watch(&watch_dir, RecursiveMode::NonRecursive)
            .map_err(|e| ReloadError::Library(e.to_string()))?;

        Ok(Self {
            watch_dir,
            watcher,
            receiver: rx,
            current: None,
        })
    }

    /// Poll for changes with timeout.
    #[must_use]
    pub fn poll(&mut self, timeout: Duration) -> Option<ReloadEvent> {
        match self.receiver.recv_timeout(timeout) {
            Ok(event) => Some(event),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => Some(ReloadEvent::Disconnected),
        }
    }

    /// Check if the current dylib has changed.
    ///
    /// # Errors
    /// Returns an error if checking fails.
    pub fn check_current(&mut self) -> ReloadResult<Option<PathBuf>> {
        let current_link = self.watch_dir.join(".current");

        if !current_link.exists() {
            return Ok(None);
        }

        let target = std::fs::read_link(&current_link)?;

        if self.current.as_ref() != Some(&target) {
            self.current = Some(target.clone());
            return Ok(Some(target));
        }

        Ok(None)
    }

    /// Get the current dylib path.
    #[must_use]
    pub fn current_path(&self) -> Option<&Path> {
        self.current.as_deref()
    }

    /// Unwatch and clean up.
    #[allow(clippy::unused_self)]
    pub fn unwatch(self) {
        // Watcher is dropped here, which unregisters it
    }
}

/// Events from the watcher.
#[derive(Debug, Clone)]
pub enum ReloadEvent {
    /// A new dylib is available
    Changed,
    /// The channel was disconnected
    Disconnected,
}
