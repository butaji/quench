// linter-skip
//! Event loop handling
//!
//! Processes keyboard, mouse, resize, timer, and microtask events.

use crate::bridge;
use crate::cli::CliArgs;
use crate::render::{keycode_to_ink_name, render_tree};
use quench_runtime::Context;
use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyModifiers, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::time::Duration;

/// Container for cached JS function references.
#[derive(Default)]
struct JsFunctions {
    dispatch_key: bool,
    dispatch_mouse: bool,
    dispatch_resize: bool,
    invoke_timers: bool,
}

/// Check if a JS function exists in the globals.
fn js_function_exists(ctx: &Context, name: &str) -> bool {
    ctx.has_function(name)
}

/// Call a JS function by name with no arguments.
#[allow(dead_code)]
fn call_js_function(ctx: &mut Context, name: &str) {
    if let Err(e) = ctx.call_function(name, vec![]) {
        tracing::warn!("Failed to call {}: {}", name, e);
    }
}

/// Main event loop (synchronous)
pub fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    _args: &CliArgs,
    _script_path: Option<String>,
    ctx: &mut Context,
) -> Result<()> {
    let mut dirty = true;
    let mut root_id = bridge::__ink_get_root_id();

    // Setup hot reload if enabled
    #[cfg(feature = "hotreload")]
    let mut hot_reloader = setup_hot_reloader(_args);

    loop {
        if should_exit() {
            break;
        }

        #[cfg(feature = "hotreload")]
        if let Some(ref mut reloader) = hot_reloader {
            if let Some(_event) = reloader.poll_changes() {
                tracing::info!("Hot reload: File changed, reloading...");
                dirty = handle_hot_reload(&_script_path, &mut root_id, std::time::Instant::now(), ctx) || dirty;
            }
        }

        let poll_timeout = compute_poll_timeout();
        if let Ok(true) = crossterm::event::poll(poll_timeout) {
            if let Ok(event) = crossterm::event::read() {
                dirty = handle_event(event, &mut root_id, ctx) || dirty;
            }
        }

        dirty = poll_timers(ctx) || dirty;

        if dirty {
            render_tree(terminal, root_id)?;
            bridge::__ink_clear_dirty();
            dirty = false;
        }
    }

    Ok(())
}

fn should_exit() -> bool {
    if bridge::__ink_should_exit() || crate::signals::shutdown_requested() {
        tracing::info!("Shutdown requested, code: {}", bridge::__ink_get_exit_code());
        return true;
    }
    false
}

/// Compute the ideal poll timeout based on the next scheduled timer.
/// 100 ms if nothing is scheduled, 1 ms if something is due in ≤1 ms,
/// otherwise the actual delay.
fn compute_poll_timeout() -> Duration {
    match bridge::__ink_next_timer_delay() {
        None => Duration::from_millis(100),
        Some(d) if d <= Duration::from_millis(1) => Duration::from_millis(1),
        Some(d) => d,
    }
}

#[cfg(feature = "hotreload")]
fn setup_hot_reloader(args: &CliArgs) -> Option<crate::hotreload::HotReloader> {
    args.watch_path.as_ref().and_then(|watch_path| {
        match crate::hotreload::HotReloader::new(watch_path) {
            Ok(hr) => {
                tracing::info!("Hot reload enabled for: {}", watch_path);
                Some(hr)
            }
            Err(e) => {
                tracing::warn!("Failed to setup hot reload: {:?}", e);
                None
            }
        }
    })
}

/// Handle a terminal event
fn handle_event(
    event: Event,
    _root_id: &mut Option<u32>,
    ctx: &mut Context,
) -> bool {
    match event {
        Event::Key(key) => handle_key_event(key, ctx),
        Event::Mouse(mouse) => handle_mouse_event(mouse, ctx),
        Event::Resize(cols, rows) => {
            tracing::trace!("Terminal resize: {}x{}", cols, rows);
            bridge::__ink_set_terminal_size(cols as u32, rows as u32);
            dispatch_resize(ctx, cols, rows)
        }
        _ => false,
    }
}

fn dispatch_resize(ctx: &mut Context, cols: u16, rows: u16) -> bool {
    // Set globals for the resize handler
    ctx.set_global("__pending_resize_cols".to_string(), quench_runtime::Value::Number(cols as f64));
    ctx.set_global("__pending_resize_rows".to_string(), quench_runtime::Value::Number(rows as f64));

    if let Err(e) = ctx.call_function("__tb_dispatch_resize", vec![]) {
        tracing::trace!("No resize handler: {}", e);
    }
    true
}

/// Handle keyboard event — dispatch to JS useInput handlers
fn handle_key_event(
    key: crossterm::event::KeyEvent,
    ctx: &mut Context,
) -> bool {
    let is_backtab = matches!(key.code, KeyCode::BackTab);
    let key_str = keycode_to_ink_name(&key);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT) || is_backtab;
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let meta = key.modifiers.contains(KeyModifiers::META)
        || key.modifiers.contains(KeyModifiers::SUPER);

    tracing::trace!("Key event: {} (ctrl={}, shift={}, alt={}, meta={})", key_str, ctrl, shift, alt, meta);

    // Handle Ctrl+C specially
    if ctrl && (key_str == "c" || key.code == KeyCode::Char('c')) {
        tracing::info!("Ctrl+C detected, initiating shutdown");
        bridge::__ink_set_exit_requested();
        return false;
    }

    // Set globals so the shim can read them
    ctx.set_global("__pending_key".to_string(), quench_runtime::Value::String(key_str));
    ctx.set_global("__pending_ctrl".to_string(), quench_runtime::Value::Boolean(ctrl));
    ctx.set_global("__pending_shift".to_string(), quench_runtime::Value::Boolean(shift));
    ctx.set_global("__pending_alt".to_string(), quench_runtime::Value::Boolean(alt));
    ctx.set_global("__pending_meta".to_string(), quench_runtime::Value::Boolean(meta));

    // Call the function
    if let Err(e) = ctx.call_function("__tb_dispatch_key", vec![]) {
        tracing::trace!("No key handler: {}", e);
    }

    bridge::__ink_is_dirty()
}

/// Handle mouse event — dispatch to JS mouse handlers
fn handle_mouse_event(
    mouse: crossterm::event::MouseEvent,
    ctx: &mut Context,
) -> bool {
    let (kind_str, button) = mouse_kind_and_button(&mouse.kind);
    let ctrl = mouse.modifiers.contains(KeyModifiers::CONTROL);
    let shift = mouse.modifiers.contains(KeyModifiers::SHIFT);
    let alt = mouse.modifiers.contains(KeyModifiers::ALT);

    tracing::trace!("Mouse event: {} at ({}, {})", kind_str, mouse.column, mouse.row);

    ctx.set_global("__pending_mouse_col".to_string(), quench_runtime::Value::Number(mouse.column as f64));
    ctx.set_global("__pending_mouse_row".to_string(), quench_runtime::Value::Number(mouse.row as f64));
    ctx.set_global("__pending_mouse_kind".to_string(), quench_runtime::Value::String(kind_str.to_string()));
    ctx.set_global("__pending_mouse_button".to_string(), quench_runtime::Value::Number(button as f64));
    ctx.set_global("__pending_mouse_ctrl".to_string(), quench_runtime::Value::Boolean(ctrl));
    ctx.set_global("__pending_mouse_shift".to_string(), quench_runtime::Value::Boolean(shift));
    ctx.set_global("__pending_mouse_alt".to_string(), quench_runtime::Value::Boolean(alt));

    // Call the function
    if let Err(e) = ctx.call_function("__tb_dispatch_mouse", vec![]) {
        tracing::trace!("No mouse handler: {}", e);
    }

    bridge::__ink_is_dirty()
}

fn mouse_kind_and_button(kind: &crossterm::event::MouseEventKind) -> (&'static str, i32) {
    let kind_str = match kind {
        MouseEventKind::Down(_) => "press",
        MouseEventKind::Up(_) => "release",
        MouseEventKind::Drag(_) | MouseEventKind::Moved => "hold",
        MouseEventKind::ScrollUp => "wheelUp",
        MouseEventKind::ScrollDown => "wheelDown",
        _ => "unknown",
    };

    let button = match kind {
        MouseEventKind::Down(btn) | MouseEventKind::Up(btn) | MouseEventKind::Drag(btn) => {
            match btn {
                crossterm::event::MouseButton::Left => 0,
                crossterm::event::MouseButton::Right => 1,
                crossterm::event::MouseButton::Middle => 2,
            }
        }
        _ => -1,
    };

    (kind_str, button)
}

/// Poll and dispatch timers
fn poll_timers(ctx: &mut Context) -> bool {
    if bridge::__ink_drain_microtasks() {
        tracing::trace!("Microtasks pending");
    }

    let timer_ids = bridge::__ink_process_timers();
    if timer_ids != "[]" {
        tracing::trace!("Timers fired: {}", timer_ids);

        // Parse timer IDs from JSON string "[1,2,3]" to Vec<u32>
        let ids: Vec<u32> = timer_ids
            .trim_matches(|c| c == '[' || c == ']')
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();

        if !ids.is_empty() {
            // Create array of timer IDs
            let timer_array = quench_runtime::Object::new_array(ids.len());
            let timer_array = std::rc::Rc::new(std::cell::RefCell::new(timer_array));
            for (i, &id) in ids.iter().enumerate() {
                timer_array.borrow_mut().set(&i.to_string(), quench_runtime::Value::Number(id as f64));
            }

            if let Err(e) = ctx.call_function("__tb_invoke_timers", vec![quench_runtime::Value::Object(timer_array)]) {
                tracing::trace!("No timer handler: {}", e);
            }
        }
    }

    bridge::__ink_is_dirty()
}

/// Handle hot reload
#[cfg(feature = "hotreload")]
fn handle_hot_reload(
    script_path: &Option<String>,
    root_id: &mut Option<u32>,
    _start: std::time::Instant,
    ctx: &mut Context,
) -> bool {
    if let Some(old_root_id) = *root_id {
        bridge::__ink_destroy_root(old_root_id);
    }

    if let Some(ref path) = script_path {
        if let Ok(new_code) = std::fs::read_to_string(path) {
            // Create a fresh context for hot reload
            match quench_runtime::Context::new() {
                Ok(mut new_ctx) => {
                    // Load runtime and new code
                    let runtime_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/runtime.js");
                    if let Err(e) = new_ctx.load_runtime_from(&runtime_path) {
                        tracing::error!("Failed to load runtime for hot reload: {:?}", e);
                        return false;
                    }
                    if let Err(e) = new_ctx.eval(&new_code) {
                        tracing::error!("Hot reload eval error: {:?}", e);
                        return false;
                    }
                    
                    // Update the main context by copying globals
                    // This is a simplified approach - in production we'd want better context management
                    let _elapsed = _start.elapsed();
                    tracing::info!("Hot reload complete in {:?}", _elapsed);

                    *root_id = bridge::__ink_get_root_id();
                    return true;
                }
                Err(e) => {
                    tracing::error!("Failed to create context for hot reload: {:?}", e);
                    return false;
                }
            }
        } else {
            tracing::error!("Hot reload: Failed to read {}", path);
        }
    }

    false
}
