//! # Watch Loop Module
//!
//! Handles the file watching loop for development mode.

use super::BuildDriver;
use crate::reload::{DylibWatcher, HostSignaler};
use crate::Result;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

impl BuildDriver {
    /// Run the development watch loop.
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

        print_watch_status(&src_dir, self.options.verbose);

        while running.load(Ordering::SeqCst) {
            let event = watcher.wait_for_event(Duration::from_millis(500));
            if let Some(reload_event) = event {
                if !process_event(reload_event, self, &signaler) {
                    break;
                }
            }
        }

        if self.options.verbose {
            println!("\nDevelopment server stopped.");
        }
        Ok(())
    }
}

fn find_src_dir(driver: &BuildDriver) -> PathBuf {
    driver
        .options
        .workspace
        .join("crates")
        .join(&driver.config.build.target_crate)
        .join("src")
}

fn create_signaler(hot_dir: &std::path::Path) -> Result<HostSignaler> {
    HostSignaler::new(hot_dir).map_err(|e| crate::RunieError::Reload(e.to_string()))
}

fn create_watcher(src_dir: &Path, debounce: u64) -> Result<DylibWatcher> {
    DylibWatcher::new(src_dir, debounce).map_err(|e| crate::RunieError::Reload(e.to_string()))
}

fn print_watch_status(src_dir: &Path, verbose: bool) {
    if verbose {
        println!("Watching for changes in {}...", src_dir.display());
        println!("Press Ctrl+C to stop.");
    }
}

fn process_event(
    event: crate::reload::ReloadEvent,
    driver: &mut BuildDriver,
    signaler: &HostSignaler,
) -> bool {
    match event {
        crate::reload::ReloadEvent::FilesChanged(_) => {
            handle_file_change(driver, signaler);
            true
        }
        crate::reload::ReloadEvent::ProtocolChanged => {
            eprintln!("Protocol changed, full restart required.");
            let _ = signaler.mark_restart_needed();
            false
        }
        crate::reload::ReloadEvent::Error(e) => {
            eprintln!("Watcher error: {}", e);
            true
        }
    }
}

fn handle_file_change(driver: &mut BuildDriver, signaler: &HostSignaler) {
    if driver.options.verbose {
        println!("File changed, rebuilding...");
    }
    match driver.build_once() {
        Ok(()) => {
            if driver.options.verbose {
                println!("Build successful, hot reload ready.");
            }
            let _ = signaler.signal();
        }
        Err(e) => {
            eprintln!("Build failed: {}", e);
        }
    }
}
