// linter-skip
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

// Our custom JavaScript runtime (from quench-runtime crate)
use quench_runtime as js_runtime;

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

    // Register host functions (FFI bridge)
    register_bridge_functions(&mut ctx);
    
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
    tracing::debug!("Setting up runtime.js");
    
    // Load runtime.js
    let runtime_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/runtime.js");
    if let Err(e) = ctx.load_runtime_from(&runtime_path) {
        tracing::error!("Runtime load error: {:?}", e);
        return Err(anyhow::anyhow!("Failed to load runtime.js: {}", e));
    }
    tracing::debug!("Runtime loaded successfully");

    // Inject bridge config
    inject_bridge_config(ctx)?;
    
    Ok(())
}

/// Register bridge FFI functions as host functions
fn register_bridge_functions(ctx: &mut js_runtime::Context) {
    use std::rc::Rc;
    use js_runtime::{Value, Object, ObjectKind};
    
    // Helper to convert value to string
    fn to_js_string(v: &Value) -> String {
        match v {
            Value::Undefined => "undefined".to_string(),
            Value::Null => "null".to_string(),
            Value::Boolean(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            Value::String(s) => s.clone(),
            Value::Object(_) => "[object Object]".to_string(),
            Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_) => "[Function]".to_string(),
            Value::Symbol(s) => format!("Symbol({})", s),
        }
    }
    
    fn to_number(v: &Value) -> f64 {
        match v {
            Value::Undefined => f64::NAN,
            Value::Null => 0.0,
            Value::Boolean(true) => 1.0,
            Value::Boolean(false) => 0.0,
            Value::Number(n) => *n,
            Value::String(s) => s.trim().parse().unwrap_or(f64::NAN),
            _ => f64::NAN,
        }
    }
    
    // __ink_call - generic bridge call
    ctx.register_native("__ink_call", move |args| {
        let method = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let args_json = args.get(1).map(|v| to_js_string(v)).unwrap_or_else(|| "[]".to_string());
        let result = crate::bridge::ffi::call_ink_ffi(&method, &args_json);
        Ok(Value::String(result))
    });
    
    // __ink_call_fast - fast path FFI
    ctx.register_native("__ink_call_fast", move |args| {
        let method_or_id = args.first().cloned().unwrap_or(Value::Undefined);
        let a = args.get(1).map(|v| to_number(v)).unwrap_or(0.0);
        let b = args.get(2).map(|v| to_number(v)).unwrap_or(0.0);
        let c = args.get(3).map(|v| to_number(v)).unwrap_or(0.0);
        let d = args.get(4).map(|v| to_number(v)).unwrap_or(0.0);
        let e = args.get(5).map(|v| to_number(v)).unwrap_or(0.0);
        
        let result = if let Value::Number(id) = method_or_id {
            crate::bridge::ffi::call_ink_ffi_fast(id as u32, a, b, c, d, e)
        } else {
            let method_name = to_js_string(&method_or_id);
            if let Some(id) = crate::bridge::ffi::get_fast_method_id(&method_name) {
                crate::bridge::ffi::call_ink_ffi_fast(id, a, b, c, d, e)
            } else {
                0.0
            }
        };
        
        Ok(Value::Number(result))
    });
    
    // Node creation functions
    ctx.register_native("__ink_create_root", move |_args| {
        let result = crate::bridge::ffi::call_ink_ffi("create_root", "[]");
        Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
    });
    
    ctx.register_native("__ink_destroy_root", move |args| {
        let id = args.first().map(|v| to_number(v) as u32).unwrap_or(0);
        let _ = crate::bridge::ffi::call_ink_ffi("destroy_root", &format!("[{}]", id));
        Ok(Value::Undefined)
    });
    
    ctx.register_native("__ink_create_node", move |args| {
        let tag = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let props = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("create_node", &format!("[\"{}\",{}]", tag, props));
        Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
    });
    
    ctx.register_native("__ink_create_text_node", move |args| {
        let text = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("create_text_node", &format!("[\"{}\"]", text));
        Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
    });
    
    // DOM manipulation
    ctx.register_native("__ink_append_child", move |args| {
        let parent = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let child = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("append_child", &format!("[{},{}]", parent, child));
        Ok(Value::Boolean(result == "true"))
    });
    
    ctx.register_native("__ink_remove_child", move |args| {
        let parent = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let child = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("remove_child", &format!("[{},{}]", parent, child));
        Ok(Value::Boolean(result == "true"))
    });
    
    ctx.register_native("__ink_insert_before", move |args| {
        let parent = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let child = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        let before = args.get(2).map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("insert_before", &format!("[{},{},{}]", parent, child, before));
        Ok(Value::Boolean(result == "true"))
    });
    
    ctx.register_native("__ink_commit_update", move |args| {
        let id = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let props = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("commit_update", &format!("[\"{}\",{}]", id, props));
        Ok(Value::Boolean(result == "true"))
    });
    
    ctx.register_native("__ink_set_text", move |args| {
        let id = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let text = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("set_text", &format!("[\"{}\",\"{}\"]", id, text));
        Ok(Value::Boolean(result == "true"))
    });
    
    ctx.register_native("__ink_commit", move |_args| {
        let _ = crate::bridge::ffi::call_ink_ffi("commit", "[]");
        Ok(Value::Undefined)
    });
    
    ctx.register_native("__ink_is_dirty", move |_args| {
        let result = crate::bridge::ffi::call_ink_ffi("is_dirty", "[]");
        Ok(Value::Boolean(result == "true"))
    });
    
    ctx.register_native("__ink_clear_dirty", move |_args| {
        let _ = crate::bridge::ffi::call_ink_ffi("clear_dirty", "[]");
        Ok(Value::Undefined)
    });
    
    // Text measurement
    ctx.register_native("__ink_measure_text", move |args| {
        let text = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let width = args.get(1).map(|v| to_number(v) as u32).unwrap_or(80);
        let result = crate::bridge::ffi::call_ink_ffi("measure_text", &format!("[\"{}\",{}]", text, width));
        
        let parts: Vec<&str> = result.split(',').collect();
        let w = parts.get(0).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let h = parts.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        
        let obj = Object::new(ObjectKind::Ordinary);
        let obj = Rc::new(std::cell::RefCell::new(obj));
        obj.borrow_mut().set("width", Value::Number(w));
        obj.borrow_mut().set("height", Value::Number(h));
        Ok(Value::Object(obj))
    });
    
    ctx.register_native("__ink_measure_element", move |args| {
        let id = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("measure_element", &format!("[{}]", id));
        
        if result == "null" {
            return Ok(Value::Null);
        }
        
        let parts: Vec<&str> = result.split(',').collect();
        let w = parts.get(0).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let h = parts.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        
        let obj = Object::new(ObjectKind::Ordinary);
        let obj = Rc::new(std::cell::RefCell::new(obj));
        obj.borrow_mut().set("width", Value::Number(w));
        obj.borrow_mut().set("height", Value::Number(h));
        Ok(Value::Object(obj))
    });
    
    // Terminal control
    ctx.register_native("__ink_exit", move |args| {
        let code = args.first().map(|v| to_number(v) as i32).unwrap_or(0);
        let _ = crate::bridge::ffi::call_ink_ffi("exit", &format!("[{}]", code));
        Ok(Value::Undefined)
    });
    
    ctx.register_native("__ink_should_exit", move |_args| {
        let result = crate::bridge::ffi::call_ink_ffi("should_exit", "[]");
        Ok(Value::Boolean(result == "true"))
    });
    
    ctx.register_native("__ink_get_exit_code", move |_args| {
        let result = crate::bridge::ffi::call_ink_ffi("get_exit_code", "[]");
        Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
    });
    
    ctx.register_native("__ink_reset_exit", move |_args| {
        let _ = crate::bridge::ffi::call_ink_ffi("reset_exit", "[]");
        Ok(Value::Undefined)
    });
    
    ctx.register_native("__ink_set_exit_requested", move |_args| {
        let _ = crate::bridge::ffi::call_ink_ffi("set_exit_requested", "[]");
        Ok(Value::Undefined)
    });
    
    ctx.register_native("__ink_set_terminal_size", move |args| {
        let width = args.first().map(|v| to_number(v) as u32).unwrap_or(80);
        let height = args.get(1).map(|v| to_number(v) as u32).unwrap_or(24);
        let _ = crate::bridge::ffi::call_ink_ffi("set_terminal_size", &format!("[{},{}]", width, height));
        Ok(Value::Undefined)
    });
    
    ctx.register_native("__ink_get_terminal_size", move |_args| {
        let result = crate::bridge::ffi::call_ink_ffi("get_terminal_size", "[]");
        let parts: Vec<&str> = result.split(',').collect();
        let w = parts.get(0).and_then(|s| s.parse::<f64>().ok()).unwrap_or(80.0);
        let h = parts.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(24.0);
        
        let obj = Object::new(ObjectKind::Ordinary);
        let obj = Rc::new(std::cell::RefCell::new(obj));
        obj.borrow_mut().set("width", Value::Number(w));
        obj.borrow_mut().set("height", Value::Number(h));
        Ok(Value::Object(obj))
    });
    
    // Node introspection
    ctx.register_native("__ink_get_node_tag", move |args| {
        let id = args.first().map(|v| to_number(v) as u32).unwrap_or(0);
        let result = crate::bridge::ffi::call_ink_ffi("get_node_tag", &format!("[{}]", id));
        if result == "null" {
            Ok(Value::Null)
        } else {
            Ok(Value::String(result.trim_matches('"').to_string()))
        }
    });
    
    ctx.register_native("__ink_get_node_text", move |args| {
        let id = args.first().map(|v| to_number(v) as u32).unwrap_or(0);
        let result = crate::bridge::ffi::call_ink_ffi("get_node_text", &format!("[{}]", id));
        if result == "null" {
            Ok(Value::Null)
        } else {
            Ok(Value::String(result.trim_matches('"').to_string()))
        }
    });
    
    ctx.register_native("__ink_get_node_children", move |args| {
        let id = args.first().map(|v| to_number(v) as u32).unwrap_or(0);
        let result = crate::bridge::ffi::call_ink_ffi("get_node_children", &format!("[{}]", id));
        
        if result == "null" {
            return Ok(Value::Null);
        }
        
        let nums: Vec<f64> = result.trim_matches(|c| c == '[' || c == ']')
            .split(',')
            .filter_map(|s| s.trim().parse::<f64>().ok())
            .collect();
        
        Ok(Value::Object(Rc::new(std::cell::RefCell::new(Object::new_array(nums.len())))))
    });
    
    ctx.register_native("__ink_get_node_prop", move |args| {
        let id = args.first().map(|v| to_number(v) as u32).unwrap_or(0);
        let prop = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("get_node_prop", &format!("[{},\"{}\"]", id, prop));
        if result == "null" {
            Ok(Value::Null)
        } else {
            Ok(Value::String(result.trim_matches('"').to_string()))
        }
    });
    
    ctx.register_native("__ink_get_root_id", move |_args| {
        let result = crate::bridge::ffi::call_ink_ffi("get_root_id", "[]");
        if result == "null" {
            Ok(Value::Null)
        } else {
            Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
        }
    });
    
    ctx.register_native("__ink_calculate_layout", move |_args| {
        let result = crate::bridge::ffi::call_ink_ffi("calculate_layout", "[]");
        Ok(Value::Boolean(result == "true"))
    });
    
    ctx.register_native("__ink_get_layout", move |args| {
        let id = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("get_layout", &format!("[{}]", id));
        
        if result == "null" {
            return Ok(Value::Null);
        }
        
        let parts: Vec<&str> = result.split(',').collect();
        let x = parts.get(0).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let y = parts.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let w = parts.get(2).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let h = parts.get(3).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        
        let obj = Object::new(ObjectKind::Ordinary);
        let obj = Rc::new(std::cell::RefCell::new(obj));
        obj.borrow_mut().set("left", Value::Number(x));
        obj.borrow_mut().set("top", Value::Number(y));
        obj.borrow_mut().set("width", Value::Number(w));
        obj.borrow_mut().set("height", Value::Number(h));
        Ok(Value::Object(obj))
    });
    
    // Timers
    ctx.register_native("__ink_set_timeout", move |args| {
        let callback = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let delay = args.get(1).map(|v| to_number(v) as u64).unwrap_or(0);
        let result = crate::bridge::ffi::call_ink_ffi("set_timeout", &format!("[\"{}\",{}]", callback, delay));
        Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
    });
    
    ctx.register_native("__ink_set_interval", move |args| {
        let callback = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let interval = args.get(1).map(|v| to_number(v) as u64).unwrap_or(0);
        let result = crate::bridge::ffi::call_ink_ffi("set_interval", &format!("[\"{}\",{}]", callback, interval));
        Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
    });
    
    ctx.register_native("__ink_clear_timer", move |args| {
        let id = args.first().map(|v| to_number(v) as u32).unwrap_or(0);
        let _ = crate::bridge::ffi::call_ink_ffi("clear_timer", &format!("[{}]", id));
        Ok(Value::Undefined)
    });
    
    // Event dispatch stubs (called from event_loop.rs via globals)
    ctx.register_native("__tb_dispatch_key", move |_args| {
        Ok(Value::Undefined)
    });
    
    ctx.register_native("__tb_dispatch_mouse", move |_args| {
        Ok(Value::Undefined)
    });
    
    ctx.register_native("__tb_dispatch_resize", move |_args| {
        Ok(Value::Undefined)
    });
    
    ctx.register_native("__tb_invoke_timers", move |_args| {
        Ok(Value::Undefined)
    });
    
    // Register Ink component tags
    ctx.set_global("Box".to_string(), Value::String("ink-box".to_string()));
    ctx.set_global("Text".to_string(), Value::String("ink-text".to_string()));
    ctx.set_global("Static".to_string(), Value::String("ink-static".to_string()));
    ctx.set_global("Newline".to_string(), Value::String("ink-newline".to_string()));
    ctx.set_global("Spacer".to_string(), Value::String("ink-spacer".to_string()));
    
    // ink namespace
    let ink_ns = Object::new(ObjectKind::Ordinary);
    let ink = Rc::new(std::cell::RefCell::new(ink_ns));
    ink.borrow_mut().set("Box", Value::String("ink-box".to_string()));
    ink.borrow_mut().set("Text", Value::String("ink-text".to_string()));
    ink.borrow_mut().set("Static", Value::String("ink-static".to_string()));
    ink.borrow_mut().set("Newline", Value::String("ink-newline".to_string()));
    ink.borrow_mut().set("Spacer", Value::String("ink-spacer".to_string()));
    ctx.set_global("ink".to_string(), Value::Object(ink));
    
    tracing::debug!("Bridge functions registered");
}

fn inject_bridge_config(ctx: &mut quench_runtime::Context) -> Result<()> {
    let bridge_config = BridgeConfig::default();
    let config_js = bridge_config.to_js_injection();
    if let Err(e) = ctx.eval(&config_js) {
        tracing::warn!("Bridge config injection error: {:?}", e);
    }
    Ok(())
}

/// Load and execute user JavaScript code
fn load_user_code(
    ctx: &mut quench_runtime::Context,
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
    ctx: &mut quench_runtime::Context,
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
