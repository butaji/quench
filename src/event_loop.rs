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
use std::io::stdout;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Main event loop (synchronous)
pub fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    args: &CliArgs,
    script_path: Option<String>,
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
        // Check for exit
        if bridge::__ink_should_exit() {
            tracing::info!("Exit requested, code: {}", bridge::__ink_get_exit_code());
            break;
        }

        // Check for hot reload changes
        #[cfg(feature = "hotreload")]
        if let Some(ref mut reloader) = hot_reloader {
            if let Some(_event) = reloader.poll_changes() {
                tracing::info!("Hot reload: File changed, reloading...");
                dirty = handle_hot_reload(&script_path, &mut root_id) || dirty;
            }
        }

        // Poll for events with a timeout
        if let Ok(true) = crossterm::event::poll(Duration::from_millis(10)) {
            if let Ok(event) = crossterm::event::read() {
                dirty = handle_event(terminal, event, &mut root_id) || dirty;
            }
        }

        // Poll timers
        dirty = poll_timers() || dirty;

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
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    event: Event,
    root_id: &mut Option<u32>,
) -> bool {
    match event {
        Event::Key(key) => handle_key_event(key),
        Event::Mouse(mouse) => handle_mouse_event(mouse),
        Event::Resize(cols, rows) => {
            tracing::debug!("Terminal resize: {}x{}", cols, rows);
            bridge::__ink_set_terminal_size(cols as u32, rows as u32);
            true
        }
        _ => false,
    }
}

/// Handle keyboard event
fn handle_key_event(key: crossterm::event::KeyEvent) -> bool {
    let is_backtab = matches!(key.code, KeyCode::BackTab);
    let key_str = keycode_to_ink_name(&key);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT) || is_backtab;
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let meta = key.modifiers.contains(KeyModifiers::META)
        || key.modifiers.contains(KeyModifiers::SUPER);

    // For now, just mark dirty - actual dispatch happens via JS callbacks
    // The JS runtime is loaded in main.rs and processes callbacks
    tracing::debug!("Key event: {} (ctrl={}, shift={}, alt={}, meta={})", key_str, ctrl, shift, alt, meta);

    bridge::__ink_is_dirty()
}

/// Handle mouse event
fn handle_mouse_event(mouse: crossterm::event::MouseEvent) -> bool {
    let kind_str = match mouse.kind {
        MouseEventKind::Down(_) => "press",
        MouseEventKind::Up(_) => "release",
        MouseEventKind::Drag(_) | MouseEventKind::Moved => "hold",
        MouseEventKind::ScrollUp => "wheelUp",
        MouseEventKind::ScrollDown => "wheelDown",
        _ => "unknown",
    };

    tracing::debug!("Mouse event: {} at ({}, {})", kind_str, mouse.column, mouse.row);

    bridge::__ink_is_dirty()
}

/// Poll and dispatch timers
fn poll_timers() -> bool {
    // Process microtasks
    if bridge::__ink_drain_microtasks() {
        tracing::debug!("Microtasks pending");
    }

    // Process timers - this is handled in Rust, callbacks stored in JS
    let timer_ids = bridge::__ink_process_timers();
    if timer_ids != "[]" {
        tracing::debug!("Timers fired: {}", timer_ids);
    }

    bridge::__ink_is_dirty()
}

/// Handle hot reload
#[cfg(feature = "hotreload")]
fn handle_hot_reload(
    script_path: &Option<String>,
    root_id: &mut Option<u32>,
) -> bool {
    // Unmount old app
    if let Some(old_root_id) = *root_id {
        bridge::__ink_destroy_root(old_root_id);
    }

    // Reload and re-execute the script
    if let Some(ref path) = script_path {
        if let Ok(new_code) = std::fs::read_to_string(path) {
            let start = std::time::Instant::now();

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

            let elapsed = start.elapsed();
            tracing::info!("Hot reload complete in {:?}", elapsed);
            if elapsed.as_millis() < 50 {
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
