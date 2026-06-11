//! Event loop handling
//!
//! Processes keyboard, mouse, resize, timer, and microtask events.

use crate::bridge;
use crate::cli::CliArgs;
use crate::render::{keycode_to_ink_name, render_tree};
use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyModifiers, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::time::Duration;

/// Main event loop (synchronous)
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
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
    let mut hot_reloader = if let Some(ref watch_path) = args.watch_path {
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
    } else {
        None
    };

    loop {
        // Check for exit or shutdown signal (Ctrl+C)
        if bridge::__ink_should_exit() || crate::signals::shutdown_requested() {
            tracing::info!("Shutdown requested, code: {}", bridge::__ink_get_exit_code());
            break;
        }

        // Check for hot reload changes
        #[cfg(feature = "hotreload")]
        if let Some(ref mut reloader) = hot_reloader {
            if let Some(_event) = reloader.poll_changes() {
                tracing::info!("Hot reload: File changed, reloading...");
                dirty = handle_hot_reload(&script_path, &mut root_id, std::time::Instant::now()) || dirty;
            }
        }

        // Poll for events with a timeout
        if let Ok(true) = crossterm::event::poll(Duration::from_millis(10)) {
            if let Ok(event) = crossterm::event::read() {
                dirty = handle_event(terminal, event, &mut root_id, js_ctx) || dirty;
            }
        }

        // Poll timers
        dirty = poll_timers(js_ctx) || dirty;

        // Render if dirty
        if dirty {
            render_tree(terminal, root_id)?;
            bridge::__ink_clear_dirty();
            dirty = false;
        }
    }

    Ok(())
}

/// Handle a terminal event
fn handle_event(
    _terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    event: Event,
    _root_id: &mut Option<u32>,
    js_ctx: &rquickjs::Context,
) -> bool {
    match event {
        Event::Key(key) => handle_key_event(key, js_ctx),
        Event::Mouse(mouse) => handle_mouse_event(mouse, js_ctx),
        Event::Resize(cols, rows) => {
            // Update the cached terminal size so the *next* render uses the
            // new dimensions, but don't force a re-render here.  Ink 4 only
            // re-renders on resize when the app subscribes via `useStdout`
            // or `useInput`; mode.tsx drives stdin itself with `node:readline`
            // and therefore sees a blank screen until the next keystroke —
            // we match that behaviour exactly so the two runtimes stay in
            // lock-step.
            tracing::debug!("Terminal resize: {}x{}", cols, rows);
            bridge::__ink_set_terminal_size(cols as u32, rows as u32);
            false
        }
        _ => false,
    }
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

    tracing::debug!("Key event: {} (ctrl={}, shift={}, alt={}, meta={})", key_str, ctrl, shift, alt, meta);

    // Handle Ctrl+C specially - in PTY/tmux, Ctrl+C is sent as 'c' with ctrl modifier
    // We need to exit the app when Ctrl+C is pressed
    if ctrl && (key_str == "c" || key.code == KeyCode::Char('c')) {
        tracing::info!("Ctrl+C detected, initiating shutdown");
        bridge::__ink_set_exit_requested();
        return false;
    }

    // Dispatch to JS __tb_dispatch_key(key, ctrl, shift, alt, meta)
    // Use JSON.stringify for safe escaping of key string
    let dispatch_js = format!(
        "if(typeof __tb_dispatch_key==='function'){{__tb_dispatch_key({}, {}, {}, {}, {})}}",
        serde_json::to_string(&key_str).unwrap_or_else(|_| "\"\"".to_string()),
        ctrl,
        shift,
        alt,
        meta,
    );

    js_ctx.with(|ctx| {
        if let Err(e) = ctx.eval::<(), _>(dispatch_js.as_str()) {
            tracing::warn!("Key dispatch error: {:?}", e);
        }
    });

    bridge::__ink_is_dirty()
}

/// Handle mouse event — dispatch to JS mouse handlers
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
fn handle_mouse_event(
    mouse: crossterm::event::MouseEvent,
    js_ctx: &rquickjs::Context,
) -> bool {
    let kind_str = match mouse.kind {
        MouseEventKind::Down(_) => "press",
        MouseEventKind::Up(_) => "release",
        MouseEventKind::Drag(_) | MouseEventKind::Moved => "hold",
        MouseEventKind::ScrollUp => "wheelUp",
        MouseEventKind::ScrollDown => "wheelDown",
        _ => "unknown",
    };

    tracing::debug!("Mouse event: {} at ({}, {})", kind_str, mouse.column, mouse.row);

    // Dispatch to JS __tb_dispatch_mouse({column, row, kind, button, ctrl, shift, alt})
    let button = match mouse.kind {
        MouseEventKind::Down(btn) | MouseEventKind::Up(btn) | MouseEventKind::Drag(btn) => {
            match btn {
                crossterm::event::MouseButton::Left => 0,
                crossterm::event::MouseButton::Right => 1,
                crossterm::event::MouseButton::Middle => 2,
            }
        }
        _ => -1,
    };
    let ctrl = mouse.modifiers.contains(KeyModifiers::CONTROL);
    let shift = mouse.modifiers.contains(KeyModifiers::SHIFT);
    let alt = mouse.modifiers.contains(KeyModifiers::ALT);

    let dispatch_js = format!(
        "if(typeof __tb_dispatch_mouse==='function'){{__tb_dispatch_mouse({{column:{},row:{},kind:'{}',button:{},ctrl:{},shift:{},alt:{}}})}}",
        mouse.column,
        mouse.row,
        kind_str,
        button,
        ctrl,
        shift,
        alt,
    );

    js_ctx.with(|ctx| {
        if let Err(e) = ctx.eval::<(), _>(dispatch_js.as_str()) {
            tracing::warn!("Mouse dispatch error: {:?}", e);
        }
    });

    bridge::__ink_is_dirty()
}

/// Poll and dispatch timers
fn poll_timers(js_ctx: &rquickjs::Context) -> bool {
    // Process microtasks
    if bridge::__ink_drain_microtasks() {
        tracing::debug!("Microtasks pending");
    }

    // Process timers - get IDs from Rust, invoke callbacks in JS
    let timer_ids = bridge::__ink_process_timers();
    if timer_ids != "[]" {
        tracing::debug!("Timers fired: {}", timer_ids);
        // Invoke the JavaScript timer callbacks
        js_ctx.with(|ctx| {
            let invoke_js = format!(
                "if(typeof __tb_invoke_timers==='function'){{__tb_invoke_timers({})}}",
                timer_ids
            );
            if let Err(e) = ctx.eval::<(), _>(invoke_js.as_str()) {
                tracing::warn!("Timer invoke error: {:?}", e);
            }
        });
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
    // Unmount old app
    if let Some(old_root_id) = *root_id {
        bridge::__ink_destroy_root(old_root_id);
    }

    // Reload and re-execute the script
    if let Some(ref path) = script_path {
        if let Ok(new_code) = std::fs::read_to_string(path) {
            // Create a new runtime for hot reload eval
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
            if _elapsed.as_millis() < 50 {
                tracing::debug!("Hot reload under 50ms target");
            }

            *root_id = bridge::__ink_get_root_id();
            return true;
        } else {
            tracing::error!("Hot reload: Failed to read {}", path);
        }
    }

    false
}
