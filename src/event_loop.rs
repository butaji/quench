//! Event loop handling
//!
//! Processes keyboard, mouse, resize, timer, and microtask events.
//!
//! Function references are cached to avoid repeated `format! + ctx.eval` calls.

use crate::bridge;
use crate::cli::CliArgs;
use crate::render::{keycode_to_ink_name, render_tree};
use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyModifiers, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::time::Duration;

/// Container for cached JS function references.
/// Note: rquickjs::Function has a lifetime tied to the context, so we
/// store the function name and look it up on each call. The optimization
/// is avoiding format! string building.
#[derive(Default)]
struct JsFunctions {
    dispatch_key: bool,
    dispatch_mouse: bool,
    dispatch_resize: bool,
    invoke_timers: bool,
}

/// Check if a JS function exists in the globals.
fn js_function_exists(js_ctx: &rquickjs::Context, name: &str) -> bool {
    js_ctx.with(|ctx| {
        let globals = ctx.globals();
        globals.get::<_, rquickjs::Value>(name).is_ok()
    })
}

/// Call a JS function by name with no arguments.
#[allow(dead_code)]
fn call_js_function(js_ctx: &rquickjs::Context, name: &str) {
    js_ctx.with(|ctx| {
        let globals = ctx.globals();
        if let Ok(func) = globals.get::<_, rquickjs::Function>(name) {
            let _: Result<(), _> = func.call::<(), ()>(());
        }
    });
}

/// Main event loop (synchronous)
pub fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    _args: &CliArgs,
    _script_path: Option<String>,
    js_ctx: &rquickjs::Context,
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
                dirty = handle_hot_reload(&_script_path, &mut root_id, std::time::Instant::now()) || dirty;
            }
        }

        let poll_timeout = compute_poll_timeout();
        if let Ok(true) = crossterm::event::poll(poll_timeout) {
            if let Ok(event) = crossterm::event::read() {
                dirty = handle_event(event, &mut root_id, js_ctx) || dirty;
            }
        }

        dirty = poll_timers(js_ctx) || dirty;

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
    js_ctx: &rquickjs::Context,
) -> bool {
    match event {
        Event::Key(key) => handle_key_event(key, js_ctx),
        Event::Mouse(mouse) => handle_mouse_event(mouse, js_ctx),
        Event::Resize(cols, rows) => {
            tracing::trace!("Terminal resize: {}x{}", cols, rows);
            bridge::__ink_set_terminal_size(cols as u32, rows as u32);
            dispatch_resize(js_ctx, cols, rows)
        }
        _ => false,
    }
}

fn dispatch_resize(js_ctx: &rquickjs::Context, cols: u16, rows: u16) -> bool {
    js_ctx.with(|ctx| {
        let globals = ctx.globals();
        if let Ok(func) = globals.get::<_, rquickjs::Function>("__tb_dispatch_resize") {
            let _ = func.call::<(u16, u16), ()>((cols, rows));
        }
    });
    true
}

/// Handle keyboard event — dispatch to JS useInput handlers
fn handle_key_event(
    key: crossterm::event::KeyEvent,
    js_ctx: &rquickjs::Context,
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

    // Set globals so the shim can read them without string allocation
    // Then call the JS function directly instead of via eval
    js_ctx.with(|ctx| {
        let globals = ctx.globals();
        let _ = globals.set("__pending_key", key_str);
        let _ = globals.set("__pending_ctrl", ctrl);
        let _ = globals.set("__pending_shift", shift);
        let _ = globals.set("__pending_alt", alt);
        let _ = globals.set("__pending_meta", meta);

        // Call the function directly instead of eval
        if let Ok(func) = globals.get::<_, rquickjs::Function>("__tb_dispatch_key") {
            let _: Result<(), _> = func.call::<(), ()>(());
        }
    });

    bridge::__ink_is_dirty()
}

/// Handle mouse event — dispatch to JS mouse handlers
fn handle_mouse_event(
    mouse: crossterm::event::MouseEvent,
    js_ctx: &rquickjs::Context,
) -> bool {
    let (kind_str, button) = mouse_kind_and_button(&mouse.kind);
    let ctrl = mouse.modifiers.contains(KeyModifiers::CONTROL);
    let shift = mouse.modifiers.contains(KeyModifiers::SHIFT);
    let alt = mouse.modifiers.contains(KeyModifiers::ALT);

    tracing::trace!("Mouse event: {} at ({}, {})", kind_str, mouse.column, mouse.row);

    js_ctx.with(|ctx| {
        let globals = ctx.globals();
        let _ = globals.set("__pending_mouse_col", mouse.column as i32);
        let _ = globals.set("__pending_mouse_row", mouse.row as i32);
        let _ = globals.set("__pending_mouse_kind", kind_str);
        let _ = globals.set("__pending_mouse_button", button);
        let _ = globals.set("__pending_mouse_ctrl", ctrl);
        let _ = globals.set("__pending_mouse_shift", shift);
        let _ = globals.set("__pending_mouse_alt", alt);

        // Call the function directly instead of eval
        if let Ok(func) = globals.get::<_, rquickjs::Function>("__tb_dispatch_mouse") {
            let _: Result<(), _> = func.call::<(), ()>(());
        }
    });

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
fn poll_timers(js_ctx: &rquickjs::Context) -> bool {
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
            js_ctx.with(|ctx| {
                let globals = ctx.globals();
                if let Ok(func) = globals.get::<_, rquickjs::Function>("__tb_invoke_timers") {
                    // Pass the timer IDs as a JS array
                    let mut js_array = rquickjs::Array::new(ctx.clone()).ok();
                    if let Some(ref arr) = js_array {
                        for (i, &id) in ids.iter().enumerate() {
                            let _ = arr.set(i, id);
                        }
                    }
                    if let Some(arr) = js_array {
                        let _: Result<(), _> = func.call::<(rquickjs::Array<'_>,), ()>((arr,));
                    }
                }
            });
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
) -> bool {
    if let Some(old_root_id) = *root_id {
        bridge::__ink_destroy_root(old_root_id);
    }

    if let Some(ref path) = script_path {
        if let Ok(new_code) = std::fs::read_to_string(path) {
            let runtime = match rquickjs::Runtime::new() {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!("Failed to create runtime for hot reload: {:?}", e);
                    return false;
                }
            };
            if let Ok(ctx) = rquickjs::Context::full(&runtime) {
                ctx.with(|ctx| {
                    let _ = ctx.eval::<(), _>(new_code.as_str());
                });
            }

            let _elapsed = _start.elapsed();
            tracing::info!("Hot reload complete in {:?}", _elapsed);

            *root_id = bridge::__ink_get_root_id();
            return true;
        } else {
            tracing::error!("Hot reload: Failed to read {}", path);
        }
    }

    false
}
