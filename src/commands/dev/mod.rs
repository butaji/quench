//! Development server with plugin-based lifecycle hooks
//!
//! Core owns: file watching, outer loop, QuickJS context
//! Plugin hooks: dev_init, dev_run_once, dev_reload

use crate::commands::build;
use crate::config::Config;
use crate::plugin;
use notify::Watcher;
use runts_plugin::{DevAction, DevContext};
use std::path::PathBuf;
use anyhow::Result;

/// Run dev server using plugin lifecycle hooks
pub async fn run_dev_server(_config: &Config, path: PathBuf, plugin_name: String) -> Result<()> {
    let plugin = plugin::get_plugin(&plugin_name)?;

    // Resolve project root - handle both absolute and relative paths
    let project_root = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(&path)
    };

    let modules = scan_modules(&project_root)?;
    let mut ctx = DevContext {
        root: project_root.clone(),
        modules,
    };

    // Run initial full build to populate .runts/build directory AND compile
    // This is required because FreshDevState::compile_project() expects
    // the build directory to exist and compiles there (it runs cargo build in .runts/build)
    tracing::info!("Running initial build...");
    if let Err(e) = build::run_full_build(_config, project_root.clone(), false).await {
        tracing::error!("Initial build failed: {}", e);
        return Err(e);
    }
    tracing::info!("Initial build complete, starting dev server...");

    let mut state = plugin.dev_init(&mut ctx)?;
    let (_watcher, tx, rx) = setup_file_watcher(&project_root)?;

    dev_loop(&*plugin, &project_root, &mut ctx, &mut *state, tx, rx)
}

fn dev_loop(
    plugin: &dyn runts_plugin::Plugin,
    project_root: &PathBuf,
    ctx: &mut DevContext,
    state: &mut dyn runts_plugin::DevState,
    tx: std::sync::mpsc::Sender<Result<notify::Event, notify::Error>>,
    rx: std::sync::mpsc::Receiver<Result<notify::Event, notify::Error>>,
) -> Result<()> {
    // Ignore events from these directories (convert to string once)
    let ignore_patterns = [".runts", "target", "node_modules", ".git"];

    loop {
        if let Ok(event) = rx.try_recv() {
            if let Some(event) = event.ok() {
                // Filter out events from build artifacts and hidden directories
                let should_reload = event.paths.iter().any(|p| {
                    let path_str = p.to_string_lossy();
                    !ignore_patterns.iter().any(|pat| path_str.contains(pat))
                });

                if should_reload {
                    ctx.modules = scan_modules(project_root)?;
                    plugin.dev_reload(ctx, state)?;
                }
            }
        }

        match plugin.dev_run_once(state)? {
            DevAction::Continue => {}
            DevAction::Stop => break,
            DevAction::Error(e) => eprintln!("Dev error: {}", e),
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    // Explicitly drop tx to signal watcher to stop
    drop(tx);
    Ok(())
}

fn setup_file_watcher(
    project_root: &PathBuf,
) -> Result<(
    notify::RecommendedWatcher,
    std::sync::mpsc::Sender<Result<notify::Event, notify::Error>>,
    std::sync::mpsc::Receiver<Result<notify::Event, notify::Error>>,
)> {
    let (tx, rx) = std::sync::mpsc::channel();
    let tx_clone = tx.clone();

    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx_clone.send(res);
    })?;

    watcher.watch(project_root, notify::RecursiveMode::Recursive)?;

    Ok((watcher, tx, rx))
}

fn scan_modules(root: &PathBuf) -> Result<Vec<String>> {
    let mut modules = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        // Skip hidden directories and build artifacts
        if path.components().any(|c| {
            let s = c.as_os_str().to_string_lossy();
            s.starts_with('.') || s == "target" || s == "node_modules"
        }) {
            continue;
        }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext.to_lowercase() == "tsx" || ext.to_lowercase() == "ts" {
                modules.push(path.to_string_lossy().to_string());
            }
        }
    }
    Ok(modules)
}
