//! Hot Reload — File watching and remount support
//!
//! This module provides hot reload capabilities:
//! - File watching via notify
//! - Remount cycle for fast feedback loop (< 50ms)

use anyhow::Result;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

/// Hot reload state
pub struct HotReloader {
    /// Channel to receive file change events
    pub event_rx: Receiver<Result<Event, notify::Error>>,
    /// The watcher handle (keeps it alive)
    _watcher: RecommendedWatcher,
}

impl HotReloader {
    /// Create a new hot reloader watching the given path
    pub fn new<P: AsRef<Path>>(watch_path: P) -> Result<Self> {
        let (tx, rx) = channel();
        
        let mut watcher = RecommendedWatcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            Config::default().with_poll_interval(Duration::from_millis(100)),
        )?;
        
        watcher.watch(watch_path.as_ref(), RecursiveMode::Recursive)?;
        
        tracing::info!("Hot reload watching: {:?}", watch_path.as_ref());
        
        Ok(Self {
            event_rx: rx,
            _watcher: watcher,
        })
    }
    
    /// Check for file changes (non-blocking)
    pub fn poll_changes(&self) -> Option<Event> {
        match self.event_rx.try_recv() {
            Ok(Ok(event)) => {
                // Only return if there are actual file modifications
                if matches!(event.kind, notify::EventKind::Create(_) | notify::EventKind::Modify(_)) {
                    tracing::debug!("File change detected: {:?}", event.paths);
                    return Some(event);
                }
                None
            }
            Ok(Err(e)) => {
                tracing::warn!("Watch error: {:?}", e);
                None
            }
            Err(_) => None,
        }
    }
}

/// Signal to trigger a hot reload
#[derive(Debug, Clone)]
pub struct ReloadSignal {
    /// Path that changed
    pub path: String,
}

/// Run hot reload loop
/// This function blocks and should be run in a separate task
pub async fn run_hot_reload(
    event_rx: Receiver<Result<Event, notify::Error>>,
    reload_tx: Sender<ReloadSignal>,
) {
    loop {
        match event_rx.recv() {
            Ok(Ok(event)) => {
                if matches!(event.kind, notify::EventKind::Create(_) | notify::EventKind::Modify(_)) {
                    for path in &event.paths {
                        tracing::info!("Hot reload triggered by: {:?}", path);
                        if reload_tx.send(ReloadSignal {
                            path: path.to_string_lossy().to_string(),
                        }).is_err() {
                            tracing::warn!("Reload channel closed, stopping hot reload");
                            return;
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                tracing::warn!("Watch error: {:?}", e);
            }
            Err(_) => {
                tracing::info!("Hot reload channel closed");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_watch_creates_event() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        
        let mut reloader = HotReloader::new(dir.path()).unwrap();
        
        // Create a file
        fs::write(&file_path, "hello").unwrap();
        
        // Poll for changes
        std::thread::sleep(Duration::from_millis(200));
        
        let event = reloader.poll_changes();
        assert!(event.is_some(), "Should detect file creation");
        
        // Clean up
        fs::remove_file(&file_path).ok();
    }
}
