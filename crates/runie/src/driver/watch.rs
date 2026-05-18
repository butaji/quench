//! # Watch Loop Module
//!
//! Handles the file watching loop for development mode.

use super::BuildDriver;
use crate::reload::{DylibWatcher, HostSignaler};
use crate::Result;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

impl BuildDriver {
    /// Run the development watch loop with cargo run.
    pub fn watch(&mut self) -> Result<()> {
        let src_dir = find_src_dir(self);
        let hot_dir = self.options.workspace.join("target/hot");
        std::fs::create_dir_all(&hot_dir)?;

        let signaler = create_signaler(&hot_dir)?;
        let watcher = create_watcher(&src_dir, self.config.dev.debounce)?;

        // Setup signal handler for graceful shutdown
        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();

        if let Err(e) = ctrlc::set_handler(move || {
            r.store(false, Ordering::SeqCst);
        }) {
            if self.options.verbose {
                eprintln!("Warning: Could not set Ctrl-C handler: {e}");
            }
        }

        // Initial build and run
        rebuild_and_run(self);

        print_watch_status(&src_dir);

        while running.load(Ordering::SeqCst) {
            let event = watcher.wait_for_event(Duration::from_millis(500));
            if let Some(reload_event) = event {
                if !process_event(reload_event, self) {
                    break;
                }
            }
        }

        println!("\nStopped.");
        Ok(())
    }
}

fn find_src_dir(driver: &BuildDriver) -> PathBuf {
    driver.options.workspace.join("src")
}

fn create_signaler(hot_dir: &std::path::Path) -> Result<HostSignaler> {
    HostSignaler::new(hot_dir).map_err(|e| crate::RunieError::Reload(e.to_string()))
}

fn create_watcher(src_dir: &Path, debounce: u64) -> Result<DylibWatcher> {
    DylibWatcher::new(src_dir, debounce).map_err(|e| crate::RunieError::Reload(e.to_string()))
}

fn print_watch_status(src_dir: &Path) {
    println!("Watching {} for changes...", src_dir.display());
    println!("Press Ctrl+C to stop.\n");
}

fn process_event(
    event: crate::reload::ReloadEvent,
    driver: &mut BuildDriver,
) -> bool {
    match event {
        crate::reload::ReloadEvent::FilesChanged(_) => {
            rebuild_and_run(driver);
            true
        }
        crate::reload::ReloadEvent::ProtocolChanged => {
            eprintln!("Protocol changed - restart required");
            false
        }
        crate::reload::ReloadEvent::Error(e) => {
            eprintln!("Watcher error: {}", e);
            true
        }
    }
}

fn rebuild_and_run(driver: &mut BuildDriver) {
    if let Err(e) = driver.build_once() {
        eprintln!("Build failed: {}", e);
        return;
    }

    // Run cargo run
    let status = Command::new("cargo")
        .args(["run"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .and_then(|mut child| child.wait());

    if let Err(e) = status {
        eprintln!("Failed to run: {}", e);
    }
}
