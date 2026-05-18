//! # Dylib Watcher
//!
//! Watches for file changes and triggers hot reload.

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use super::{ReloadError, ReloadResult};

/// Events emitted by the dylib watcher.
#[derive(Debug, Clone)]
pub enum ReloadEvent {
    /// Files changed, rebuild needed
    FilesChanged(Vec<PathBuf>),
    /// Protocol changed, full restart needed
    ProtocolChanged,
    /// Error occurred
    Error(String),
}

/// Watches for file changes in Rune source files.
pub struct DylibWatcher {
    /// Watcher for file system events
    #[allow(dead_code)]
    watcher: RecommendedWatcher,
    /// Receiver for events
    receiver: Receiver<Result<Event, notify::Error>>,
    /// Directory being watched
    watched_dir: PathBuf,
    /// Debounce duration
    #[allow(dead_code)]
    debounce: Duration,
}

impl DylibWatcher {
    /// Create a new watcher for a directory.
    ///
    /// # Errors
    /// Returns an error if the watcher cannot be created.
    pub fn new(dir: &Path, debounce_ms: u64) -> ReloadResult<Self> {
        let (tx, rx) = channel();
        let debounce = Duration::from_millis(debounce_ms);

        let watcher = RecommendedWatcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            Config::default().with_poll_interval(debounce),
        )
        .map_err(|e| ReloadError::Library(e.to_string()))?;

        let mut watcher = watcher;
        watcher
            .watch(dir, RecursiveMode::Recursive)
            .map_err(|e| ReloadError::Library(e.to_string()))?;

        Ok(Self {
            watcher,
            receiver: rx,
            watched_dir: dir.to_path_buf(),
            debounce,
        })
    }

    /// Check for any pending events.
    #[must_use]
    pub fn poll(&self) -> Option<ReloadEvent> {
        match self.receiver.try_recv() {
            Ok(Ok(event)) => self.process_event(event),
            Ok(Err(e)) => Some(ReloadEvent::Error(e.to_string())),
            Err(_) => None,
        }
    }

    /// Wait for an event with timeout.
    #[must_use]
    pub fn wait_for_event(&self, timeout: Duration) -> Option<ReloadEvent> {
        match self.receiver.recv_timeout(timeout) {
            Ok(Ok(event)) => self.process_event(event),
            Ok(Err(e)) => Some(ReloadEvent::Error(e.to_string())),
            Err(_) => None,
        }
    }

    /// Get the watched directory.
    #[must_use]
    pub fn watched_dir(&self) -> &Path {
        &self.watched_dir
    }

    /// Process a file system event.
    fn process_event(&self, event: Event) -> Option<ReloadEvent> {
        let paths: Vec<PathBuf> = event
            .paths
            .into_iter()
            .filter(|p| {
                p.extension()
                    .is_some_and(|e| e == "r.ts" || e == "r.tsx" || e == "rs")
            })
            .collect();

        if paths.is_empty() {
            return None;
        }

        let protocol_changed = paths
            .iter()
            .any(|p| p.to_string_lossy().contains("protocol"));

        if protocol_changed {
            Some(ReloadEvent::ProtocolChanged)
        } else {
            Some(ReloadEvent::FilesChanged(paths))
        }
    }
}
