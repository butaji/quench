//! Development server with plugin-based lifecycle hooks
//!
//! Core owns: file watching, outer loop, QuickJS context
//! Plugin hooks: dev_init, dev_run_once, dev_reload

use crate::commands::build;
use crate::config::Config;
use crate::plugin;
use anyhow::{Context, Result};
use notify::Watcher;
use runts_plugin::{DevAction, DevContext};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Run dev server using plugin lifecycle hooks
pub async fn run_dev_server(
    _config: &Config,
    path: PathBuf,
    plugin_name: String,
    once: bool,
) -> Result<()> {
    let project_root = resolve_project_root(&path)?;

    if plugin_name == "ratatui" || plugin_name == "ink" {
        if once {
            let output = render_ink_project(&project_root)?;
            println!("{}", output);
            return Ok(());
        }
        return run_ink_watch(&project_root);
    }

    let plugin = plugin::get_plugin(&plugin_name)?;
    let modules = scan_modules(&project_root)?;
    let has_tsx = modules.iter().any(|m| m.ends_with(".tsx"));
    let mut ctx = DevContext {
        root: project_root.clone(),
        modules,
    };

    run_initial_build(_config, &project_root, has_tsx, &plugin_name).await?;

    let mut state = plugin.dev_init(&mut ctx)?;
    let (_watcher, tx, rx) = setup_file_watcher(&project_root)?;

    dev_loop(&*plugin,
        &project_root,
        &mut ctx,
        &mut *state,
        tx,
        rx,
    )
}

fn resolve_project_root(path: &PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.clone())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

async fn run_initial_build(
    _config: &Config,
    project_root: &PathBuf,
    has_tsx: bool,
    plugin_name: &str,
) -> Result<()> {
    if !has_tsx || plugin_name != "fresh" {
        return Ok(());
    }
    tracing::info!("Running initial build...");
    match build::run_full_build(_config, project_root.clone(), false).await {
        Ok(_) => {
            tracing::info!("Initial build complete, starting dev server...");
            Ok(())
        }
        Err(e) => {
            tracing::error!("Initial build failed: {}", e);
            Err(e).context("Initial build failed")
        }
    }
}

fn dev_loop(
    plugin: &dyn runts_plugin::Plugin,
    project_root: &PathBuf,
    ctx: &mut DevContext,
    state: &mut dyn runts_plugin::DevState,
    tx: std::sync::mpsc::Sender<Result<notify::Event, notify::Error>>,
    rx: std::sync::mpsc::Receiver<Result<notify::Event, notify::Error>>,
) -> Result<()> {
    let ignore_dirs = [".runts", "target", "node_modules", ".git"];

    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(event)) => {
                let should_reload = event.paths.iter().any(|p| {
                    !p.components().any(|c| {
                        let s = c.as_os_str().to_string_lossy();
                        ignore_dirs.iter().any(|dir| s == *dir)
                    })
                });

                if should_reload {
                    ctx.modules = scan_modules(project_root)?;
                    plugin.dev_reload(ctx, state)?;
                }
            }
            Ok(Err(e)) => {
                eprintln!("File watcher error: {}", e);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }

        match plugin.dev_run_once(state)? {
            DevAction::Continue => {}
            DevAction::Stop => break,
            DevAction::Error(e) => eprintln!("Dev error: {}", e),
        }
    }
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

/* -------------------------------------------------------------------------- */
/* Ink / Ratatui rquickjs dev path                                            */
/* -------------------------------------------------------------------------- */

fn render_ink_project(project_root: &Path) -> Result<String> {
    let app_tsx = find_app_tsx(project_root)?;
    let source = std::fs::read_to_string(&app_tsx)
        .with_context(|| format!("Failed to read {}", app_tsx.display()))?;
    let js = crate::transpile::js_bundle::transpile_to_js(&source)?;
    eval_ink_bundle_and_render(&js)
}

fn find_app_tsx(project_root: &Path) -> Result<PathBuf> {
    let candidates = [
        project_root.join("tui").join("app.tsx"),
        project_root.join("app.tsx"),
        project_root.join("main.tsx"),
    ];
    for c in &candidates {
        if c.exists() {
            return Ok(c.clone());
        }
    }
    for entry in walkdir::WalkDir::new(project_root)
        .max_depth(2)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext == "tsx" {
                return Ok(path.to_path_buf());
            }
        }
    }
    anyhow::bail!("No .tsx file found in {}", project_root.display())
}

fn eval_ink_bundle_and_render(js: &str) -> Result<String> {
    let runtime = rquickjs::Runtime::new()
        .map_err(|e| anyhow::anyhow!("Failed to create runtime: {:?}", e))?;
    let ctx = rquickjs::Context::full(&runtime)
        .map_err(|e| anyhow::anyhow!("Failed to create context: {:?}", e))?;

    let rendered = ctx
        .with(|ctx| eval_bundle_in_ctx(&ctx, js))
        .map_err(|e| anyhow::anyhow!("QuickJS error: {:?}", e))?;

    Ok(rendered)
}

fn eval_bundle_in_ctx(
    ctx: &rquickjs::Ctx,
    js: &str,
) -> anyhow::Result<String> {
    setup_ink_ctx(&ctx)?;
    ctx.eval::<rquickjs::Value, _>(js)
        .map_err(|e| anyhow::anyhow!("Bundle eval failed: {:?}", e))?;
    let output: String = ctx
        .eval("runts_ink.render_to_string(__runts_default({}));").map_err(|e| anyhow::anyhow!("Render failed: {:?}", e))?;
    Ok(output)
}

fn setup_ink_ctx(ctx: &rquickjs::Ctx) -> anyhow::Result<()> {
    let globals = ctx.globals();
    let print_fn = rquickjs::Function::new(ctx.clone(), |msg: String| {
        eprint!("{}", msg);
    })
    .map_err(|e| anyhow::anyhow!("Failed to create print fn: {:?}", e))?;
    globals
        .set("__runts_stderr__", print_fn)
        .map_err(|e| anyhow::anyhow!("Failed to set __runts_stderr__: {:?}", e))?;
    runts_ink::js_bridge::install(&ctx)
        .map_err(|e| anyhow::anyhow!("Failed to install ink bridge: {:?}", e))?;
    Ok(())
}

fn run_ink_watch(project_root: &Path) -> Result<()> {
    let (_watcher, _tx, rx) = setup_file_watcher(&project_root.to_path_buf())?;
    let ignore_dirs = [".runts", "target", "node_modules", ".git"];

    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(event)) => handle_watch_event(project_root, &event, &ignore_dirs),
            Ok(Err(e)) => eprintln!("File watcher error: {}", e),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

fn handle_watch_event(
    project_root: &Path,
    event: &notify::Event,
    ignore_dirs: &[&str],
) {
    if should_reload_ink(event, ignore_dirs) {
        match render_ink_project(project_root) {
            Ok(output) => println!("{}", output),
            Err(e) => eprintln!("Render error: {}", e),
        }
    }
}

fn should_reload_ink(event: &notify::Event, ignore_dirs: &[&str]) -> bool {
    event.paths.iter().any(|p| {
        p.extension().and_then(|e| e.to_str()) == Some("tsx")
            && !p.components().any(|c| {
                let s = c.as_os_str().to_string_lossy();
                ignore_dirs.iter().any(|dir| s == *dir)
            })
    })
}
