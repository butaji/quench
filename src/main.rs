//! TuiBridge — Run Ink (React for terminals) using rquickjs + Rust
//!
//! This binary loads a bundled JavaScript application (Ink-compatible)
//! and executes it in a QuickJS runtime with Yoga layout and ratatui rendering.

#![deny(unused_must_use)]
#![deny(clippy::all)]
#![allow(clippy::upper_case_acronyms)]
#![allow(dead_code)]

mod ink;
mod bridge;
mod ink_js;
mod compat;
mod bridge_config;
#[cfg(feature = "hotreload")]
mod hotreload;
#[cfg(feature = "compiler")]
mod compiler;

mod cli;
mod event_loop;
mod render;
mod signals;

#[cfg(feature = "bench")]
mod bench;

use anyhow::Result;
use bridge_config::BridgeConfig;
use cli::{parse_args, handle_compiler_cmd, compile_in_memory};
use render::render_tree;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn setup_logging() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tuibridge=debug"));

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(true).with_writer(std::io::stderr))
        .with(filter)
        .init();
}

fn main() -> Result<()> {
    setup_logging();
    tracing::info!("TuiBridge v{} — Ink runtime in rquickjs + Rust", VERSION);

    // Must be set before terminal init so cleanup runs even on Ctrl+C
    signals::setup_signal_handlers();

    let args: Vec<String> = std::env::args().collect();
    let cli_args = parse_args(&args);

    if try_handle_compiler_cmd(&cli_args) {
        return Ok(());
    }

    tracing::debug!("Initializing QuickJS runtime");
    let runtime = rquickjs::Runtime::new()?;
    let ctx = rquickjs::Context::full(&runtime)?;
    setup_runtime(&ctx)?;

    let (final_cli_args, root_id) = compile_and_load(&ctx, cli_args)?;

    let is_tty = atty::is(atty::Stream::Stdout);
    if !is_tty {
        tracing::info!("Not a TTY, skipping terminal initialization");
        return Ok(());
    }

    setup_terminal(&ctx, final_cli_args, root_id)?;

    // Normal exit path already calls disable_raw_mode / show_cursor.
    // Signal handler sets SHUTDOWN_REQUESTED so event_loop can drain gracefully.
    Ok(())
}

/// Handle standalone compiler commands (non in-memory)
fn try_handle_compiler_cmd(cli_args: &cli::CliArgs) -> bool {
    if let Some(cmd) = &cli_args.compiler_cmd {
        if !matches!(cmd, cli::CompilerCmd::CompileInMemory { .. }) {
            handle_compiler_cmd(cmd.clone());
            return true;
        }
    }
    false
}

/// Compile TSX/JSX in-memory and load user code
fn compile_and_load(
    ctx: &rquickjs::Context,
    mut cli_args: cli::CliArgs,
) -> Result<(cli::CliArgs, Option<u32>)> {
    if let Some(cli::CompilerCmd::CompileInMemory { input }) = cli_args.compiler_cmd.take() {
        tracing::info!("Compiling {} in-memory...", input);
        match compile_in_memory(&input) {
            Ok(js) => {
                tracing::debug!("Compiled {} -> {} chars", input, js.len());
                cli_args.js_code = Some(js);
                cli_args.script = None;
            }
            Err(e) => {
                tracing::error!("Compilation error: {}", e);
                std::process::exit(1);
            }
        }
    }

    let root_id = load_user_code(ctx, &cli_args)?;
    Ok((cli_args, root_id))
}

/// Setup the QuickJS runtime with bridge functions and runtime.js
fn setup_runtime(ctx: &rquickjs::Context) -> Result<()> {
    register_ink_api(ctx);
    register_ink_call(ctx);
    load_runtime_js(ctx);
    inject_bridge_config(ctx);
    Ok(())
}

fn register_ink_api(ctx: &rquickjs::Context) {
    ctx.with(|ctx| {
        if let Err(e) = ink_js::register(ctx) {
            tracing::warn!("ink_js::register error: {:?}", e);
        }
    });
}

fn register_ink_call(ctx: &rquickjs::Context) {
    ctx.with(|ctx| {
        let globals = ctx.globals();
        let ink_call = rquickjs::Function::new(
            ctx.clone(),
            |method: String, args_json: String| -> String {
                bridge::call_ink_ffi(&method, &args_json)
            },
        );
        if let Err(e) = globals.set("__ink_call", ink_call) {
            tracing::warn!("Failed to set __ink_call: {:?}", e);
        } else {
            tracing::debug!("__ink_call registered");
        }
    });
}

fn load_runtime_js(ctx: &rquickjs::Context) {
    let runtime_js = include_str!("runtime.js");
    ctx.with(|ctx| {
        if let Err(e) = ctx.eval::<(), _>(runtime_js) {
            tracing::error!("Runtime load error: {:?}", e);
        } else {
            tracing::debug!("Runtime loaded successfully");
        }
    });
}

fn inject_bridge_config(ctx: &rquickjs::Context) {
    let bridge_config = BridgeConfig::default();
    let config_js = bridge_config.to_js_injection();
    ctx.with(|ctx| {
        if let Err(e) = ctx.eval::<(), _>(config_js.as_str()) {
            tracing::warn!("Bridge config injection error: {:?}", e);
        }
    });
}

/// Load and execute user JavaScript code
fn load_user_code(
    ctx: &rquickjs::Context,
    cli_args: &cli::CliArgs,
) -> Result<Option<u32>> {
    if let Some(ref script) = cli_args.script {
        tracing::debug!("Loading script: {}", script);
        let code = std::fs::read_to_string(script)?;
        ctx.with(|ctx| {
            if let Err(_e) = ctx.eval::<(), _>(&*code) {
                // Call catch() to clear any pending exception
                // This handles cases where JS code uses try-catch internally
                let _ = ctx.catch();
                // Note: We intentionally don't log the error since JS caught it
            }
        });
    }

    if let Some(ref code) = cli_args.js_code {
        ctx.with(|ctx| {
            if let Err(_e) = ctx.eval::<(), _>(code.as_str()) {
                // Call catch() to clear any pending exception
                // This handles cases where JS code uses try-catch internally
                let _ = ctx.catch();
                // Note: We intentionally don't log the error since JS caught it
            }
        });
    }

    let root_id = bridge::__ink_get_root_id();
    tracing::info!("Root node: {:?}", root_id);
    Ok(root_id)
}

/// Setup terminal and run event loop
fn setup_terminal(
    ctx: &rquickjs::Context,
    cli_args: cli::CliArgs,
    root_id: Option<u32>,
) -> Result<()> {
    use std::io::stdout;

    // Try to enable raw mode
    if crossterm::terminal::enable_raw_mode().is_err() {
        tracing::warn!("Could not enable raw mode");
        return Ok(());
    }

    let mut terminal = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(stdout()))?;
    let _ = terminal.hide_cursor();
    let _ = terminal.clear();

    // Get script path for hot reload
    let script_path = cli_args.script.clone();

    // Run event loop or single render
    if cli_args.interactive {
        // Run event loop
        event_loop::run_event_loop(&mut terminal, &cli_args, script_path, ctx)?;
    } else {
        // Non-interactive: single render and exit
        if let Err(e) = render_tree(&mut terminal, root_id) {
            tracing::error!("Render error: {:?}", e);
        }
    }

    // Cleanup
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = terminal.show_cursor();

    tracing::info!("TuiBridge shutting down");

    // Force exit to bypass rquickjs GC assertion
    std::process::exit(0);
}
