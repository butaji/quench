//! Quench — Run Ink (React for terminals) using custom JS runtime + Rust
//!
//! This binary loads a bundled JavaScript application (Ink-compatible)
//! and executes it in a custom recursive-descent interpreter with Yoga layout
//! and ratatui rendering.

#![deny(unused_must_use)]
#![allow(clippy::upper_case_acronyms)]
#![allow(dead_code)]

mod ink;
mod bridge;
mod ink_js;
mod bridge_config;
#[cfg(feature = "hotreload")]
mod hotreload;
mod compiler;

mod cli;
mod event_loop;
mod render;
mod signals;

// Our custom JavaScript runtime
mod js_runtime;

use anyhow::Result;
use bridge_config::BridgeConfig;
use cli::{parse_args, handle_compiler_cmd, compile_in_memory};
use render::render_tree;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Get terminal size, checking COLUMNS/ROWS env vars first
fn get_terminal_size() -> (u32, u32) {
    let cols = std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok());
    let rows = std::env::var("ROWS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok());
    
    if let (Some(cols), Some(rows)) = (cols, rows) {
        return (cols, rows);
    }
    
    let (c, r) = crossterm::terminal::size().unwrap_or((80, 24));
    (c as u32, r as u32)
}

fn setup_logging() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,quench=info"));

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(true).with_writer(std::io::stderr))
        .with(filter)
        .init();
}

fn main() -> Result<()> {
    setup_logging();
    tracing::info!("Quench v{} — Ink runtime in custom JS + Rust", VERSION);

    // Install panic hook FIRST, then signal handlers, then anything
    // terminal-related.  If signal-handler installation itself panics,
    // the panic hook must already be in place so cleanup still runs.
    signals::install_panic_cleanup();
    signals::setup_signal_handlers();

    let args: Vec<String> = std::env::args().collect();
    let cli_args = parse_args(&args);

    if try_handle_compiler_cmd(&cli_args) {
        return Ok(());
    }

    run_app(cli_args)
}

fn run_app(cli_args: cli::CliArgs) -> Result<()> {
    let mut ctx = init_runtime()?;
    let (final_cli_args, root_id) = compile_and_load(&mut ctx, cli_args)?;

    if !atty::is(atty::Stream::Stdout) {
        tracing::info!("Not a TTY, skipping terminal initialization");
        return Ok(());
    }

    setup_terminal(&mut ctx, final_cli_args, root_id)?;
    Ok(())
}

fn init_runtime() -> Result<js_runtime::Context> {
    tracing::debug!("Initializing custom JS runtime");
    let mut ctx = js_runtime::Context::new()?;
    
    // Set terminal size in bridge
    let (cols, rows) = get_terminal_size();
    bridge::__ink_set_terminal_size(cols, rows);

    // Setup the runtime
    setup_runtime(&mut ctx)?;
    Ok(ctx)
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
    ctx: &mut js_runtime::Context,
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

/// Setup the runtime with runtime.js
fn setup_runtime(ctx: &mut js_runtime::Context) -> Result<()> {
    // Register host functions (FFI bridge) first
    js_runtime::host::register_host_functions(ctx);
    tracing::debug!("Host functions registered");
    
    // Load runtime.js
    if let Err(e) = ctx.load_runtime() {
        tracing::error!("Runtime load error: {:?}", e);
        return Err(anyhow::anyhow!("Failed to load runtime.js: {}", e));
    }
    tracing::debug!("Runtime loaded successfully");

    // Inject bridge config
    inject_bridge_config(ctx)?;
    
    Ok(())
}

fn inject_bridge_config(ctx: &mut js_runtime::Context) -> Result<()> {
    let bridge_config = BridgeConfig::default();
    let config_js = bridge_config.to_js_injection();
    if let Err(e) = ctx.eval(&config_js) {
        tracing::warn!("Bridge config injection error: {:?}", e);
    }
    Ok(())
}

/// Load and execute user JavaScript code
fn load_user_code(
    ctx: &mut js_runtime::Context,
    cli_args: &cli::CliArgs,
) -> Result<Option<u32>> {
    if let Some(ref script) = cli_args.script {
        tracing::debug!("Loading script: {}", script);
        let code = std::fs::read_to_string(script)?;
        if let Err(e) = ctx.eval(&code) {
            tracing::warn!("Script error (may be caught by JS): {:?}", e);
        }
    }

    if let Some(ref code) = cli_args.js_code {
        if let Err(e) = ctx.eval(code) {
            tracing::error!("JS code error: {:?}", e);
        }
    }

    let root_id = bridge::__ink_get_root_id();
    tracing::info!("Root node: {:?}", root_id);
    Ok(root_id)
}

/// Setup terminal and run event loop
fn setup_terminal(
    ctx: &mut js_runtime::Context,
    cli_args: cli::CliArgs,
    root_id: Option<u32>,
) -> Result<()> {
    use std::io::stdout;

    // Try to enable raw mode
    if crossterm::terminal::enable_raw_mode().is_err() {
        tracing::warn!("Could not enable raw mode");
        return Ok(());
    }
    crate::bridge::io::set_raw_mode_tracking(true);
    crate::signals::set_terminal_mode("raw", true);

    let mut terminal = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(stdout()))?;
    let _ = terminal.hide_cursor();
    let _ = terminal.clear();

    // Get script path for hot reload
    let script_path = cli_args.script.clone();

    // Run event loop or single render
    let run_result: Result<()> = if cli_args.interactive {
        event_loop::run_event_loop(&mut terminal, &cli_args, script_path, ctx)
    } else {
        if let Err(e) = render_tree(&mut terminal, root_id) {
            tracing::error!("Render error: {:?}", e);
        }
        tracing::info!("Single render complete");
        Ok(())
    };

    // Comprehensive cleanup.  We restore every terminal mode the user might
    // have observed in our TUI (raw, cursor, mouse, alt screen, bracketed
    // paste) so their shell stays in a sane state after we exit.
    if let Err(e) = run_result {
        tracing::warn!("Event loop error: {:?}", e);
    }
    cleanup_terminal(&mut terminal);

    // Exit cleanly
    std::process::exit(0);
}

/// Restore the terminal to the state it was in before we took over.  Each
/// step is best-effort: a failure here must not prevent the others from
/// running.
fn cleanup_terminal(terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>) {
    use std::io::Write;
    let modes = crate::signals::get_terminal_modes();
    let mut out = std::io::stdout();

    if modes.raw_mode {
        let _ = crossterm::terminal::disable_raw_mode();
    }
    crate::bridge::io::set_raw_mode_tracking(false);
    let _ = terminal.show_cursor();
    if modes.mouse_tracking {
        let _ = out.write_all(b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l");
    }
    if modes.alt_screen {
        let _ = out.write_all(b"\x1b[?1049l");
    }
    if modes.bracketed_paste {
        let _ = out.write_all(b"\x1b[?2004l");
    }
    let _ = out.write_all(b"\x1b[?25h");
    let _ = out.flush();
}
