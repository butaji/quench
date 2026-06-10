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

use anyhow::Result;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io::stdout;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn setup_logging() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tuibridge=debug"));

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(true))
        .with(filter)
        .init();
}

/// Render the current tree to the terminal
fn render_tree(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    root_id: Option<u32>,
) -> Result<()> {
    terminal.draw(|frame| {
        let Some(root_id) = root_id else { return; };
        
        let area = frame.area();
        
        // Set terminal size for layout calculation
        bridge::__ink_set_terminal_size(area.width as u32, area.height as u32);
        
        // Calculate layout
        if let Err(e) = bridge::__ink_calculate_layout() {
            tracing::error!("Layout error: {:?}", e);
            return;
        }
        
        // Render nodes recursively
        render_node(root_id, frame.buffer_mut(), area);
    })?;
    
    Ok(())
}

/// Recursively render a node and its children
fn render_node(
    node_id: u32,
    buf: &mut ratatui::buffer::Buffer,
    area: ratatui::layout::Rect,
) {
    use ratatui::widgets::Widget;
    use ratatui::style::Style;
    use ratatui::layout::Rect;
    use crate::ink::PropValue;
    
    let tag = match bridge::__ink_get_node_tag(node_id) {
        Some(t) => t,
        None => return,
    };
    
    let layout = match bridge::__ink_get_layout(node_id) {
        Some(l) => l,
        None => return,
    };
    
    // Ink uses round() for positions and ceil() for dimensions to avoid clipping
    let x = layout.0.round() as u16;
    let y = layout.1.round() as u16;
    let w = layout.2.ceil() as u16;
    let h = layout.3.ceil() as u16;
    
    // Skip if out of bounds
    if x >= area.right() || y >= area.bottom() {
        return;
    }
    
    match tag.as_str() {
        "ink-box" => {
            use ratatui::widgets::Block;
            use ratatui::widgets::{Borders, BorderType};
            
            // Check for border style
            let border_style = bridge::__ink_get_node_prop(node_id, "borderStyle")
                .map(|s| s.trim_matches('"').to_string());
            
            let mut block = Block::default();
            
            // Check for individual border sides (boolean props in Ink)
            let border_top = matches!(
                bridge::__ink_get_node_prop_raw(node_id, "borderTop"),
                Some(PropValue::Bool(true))
            );
            let border_bottom = matches!(
                bridge::__ink_get_node_prop_raw(node_id, "borderBottom"),
                Some(PropValue::Bool(true))
            );
            let border_left = matches!(
                bridge::__ink_get_node_prop_raw(node_id, "borderLeft"),
                Some(PropValue::Bool(true))
            );
            let border_right = matches!(
                bridge::__ink_get_node_prop_raw(node_id, "borderRight"),
                Some(PropValue::Bool(true))
            );

            // Check for border color (applied to all borders)
            let border_color = bridge::__ink_get_node_prop(node_id, "borderColor")
                .map(|s| s.trim_matches('"').to_string())
                .and_then(|s| parse_color(&s));
            let border_dim_color = bridge::__ink_get_node_prop(node_id, "borderDimColor")
                .map(|s| s.trim_matches('"').to_string())
                .and_then(|s| parse_color(&s));

            let has_individual_borders = border_top || border_bottom || border_left || border_right;

            if has_individual_borders || border_style.is_some() {
                let border_type = border_style.as_ref().map(|s| match s.as_str() {
                    "round" => BorderType::Rounded,
                    "bold" => BorderType::Thick,
                    "double" => BorderType::Double,
                    _ => BorderType::Plain,
                }).unwrap_or(BorderType::Plain);

                let borders = if has_individual_borders {
                    let mut b = Borders::empty();
                    if border_top { b.insert(Borders::TOP); }
                    if border_bottom { b.insert(Borders::BOTTOM); }
                    if border_left { b.insert(Borders::LEFT); }
                    if border_right { b.insert(Borders::RIGHT); }
                    b
                } else {
                    Borders::ALL
                };

                block = block.borders(borders).border_type(border_type);

                // Apply border color + dim modifier
                let mut border_sty = Style::default();
                if let Some(color) = border_color {
                    border_sty = border_sty.fg(color);
                }
                if border_dim_color.is_some() {
                    border_sty = border_sty.add_modifier(ratatui::style::Modifier::DIM);
                }
                if border_sty != Style::default() {
                    block = block.border_style(border_sty);
                }
            }
            
            // Add padding if specified
            if let Some(PropValue::Number(padding)) = bridge::__ink_get_node_prop_raw(node_id, "padding") {
                let p = padding as u16;
                if p > 0 {
                    block = block.padding(ratatui::widgets::Padding::symmetric(p, p));
                }
            } else if let (Some(PropValue::Number(py)), Some(PropValue::Number(px))) = (
                bridge::__ink_get_node_prop_raw(node_id, "paddingY"),
                bridge::__ink_get_node_prop_raw(node_id, "paddingX")
            ) {
                block = block.padding(ratatui::widgets::Padding::symmetric(py as u16, px as u16));
            }
            
            // Add title if present
            if let Some(title) = bridge::__ink_get_node_prop(node_id, "title")
                .map(|s| s.trim_matches('"').to_string()) {
                block = block.title(title);
            }
            
            // Check for background color
            let mut bg_style = Style::default();
            if let Some(bg_color) = bridge::__ink_get_node_prop(node_id, "backgroundColor")
                .map(|s| s.trim_matches('"').to_string())
                .and_then(|s| parse_color(&s))
            {
                bg_style = bg_style.bg(bg_color);
            }
            
            let rect = Rect::new(x, y, w, h);

            // Apply background color to block style so borders inherit it
            if let Some(bg) = bg_style.bg {
                block = block.style(Style::default().bg(bg));
                // Also fill inner area (Block doesn't fill inner background)
                for cy in rect.y..rect.bottom() {
                    for cx in rect.x..rect.right() {
                        if cx < buf.area.right() && cy < buf.area.bottom() {
                            if let Some(cell) = buf.cell_mut((cx, cy)) {
                                cell.set_bg(bg);
                            }
                        }
                    }
                }
            }

            block.render(rect, buf);
        }
        
        "ink-text" => {
            use ratatui::widgets::Paragraph;
            
            let text = bridge::__ink_get_node_text(node_id).unwrap_or_default();
            
            // Check for color prop
            let mut style = Style::default();
            if let Some(color) = bridge::__ink_get_node_prop(node_id, "color")
                .map(|s| s.trim_matches('"').to_string()) {
                if let Some(c) = parse_color(&color) {
                    style = style.fg(c);
                }
            }
            
            // Check for background color (critical for selection highlighting)
            // We use style.bg() which sets the background for the entire widget area
            if let Some(bg_color) = bridge::__ink_get_node_prop(node_id, "backgroundColor")
                .map(|s| s.trim_matches('"').to_string())
                .and_then(|s| parse_color(&s))
            {
                style = style.bg(bg_color);
            }
            
            // Check for bold
            if bridge::__ink_get_node_prop(node_id, "bold").is_some() {
                style = style.add_modifier(ratatui::style::Modifier::BOLD);
            }
            
            // Check for dim
            if bridge::__ink_get_node_prop(node_id, "dimColor").is_some() {
                style = style.add_modifier(ratatui::style::Modifier::DIM);
            }
            
            // Check for italic
            if bridge::__ink_get_node_prop(node_id, "italic").is_some() {
                style = style.add_modifier(ratatui::style::Modifier::ITALIC);
            }
            
            // Check for strikethrough
            if bridge::__ink_get_node_prop(node_id, "strikethrough").is_some() {
                style = style.add_modifier(ratatui::style::Modifier::CROSSED_OUT);
            }
            
            // Check for underline
            if bridge::__ink_get_node_prop(node_id, "underline").is_some() {
                style = style.add_modifier(ratatui::style::Modifier::UNDERLINED);
            }
            
            // Check for inverse
            if bridge::__ink_get_node_prop(node_id, "inverse").is_some() {
                style = style.add_modifier(ratatui::style::Modifier::REVERSED);
            }
            
            // Check for text transform (uppercase/lowercase)
            let text = if let Some(transform) = bridge::__ink_get_node_prop(node_id, "transform")
                .map(|s| s.trim_matches('"').to_string())
            {
                match transform.as_str() {
                    "uppercase" => text.to_uppercase(),
                    "lowercase" => text.to_lowercase(),
                    _ => text,
                }
            } else {
                text
            };
            
            let para = Paragraph::new(text.as_str())
                .style(style);
            
            let rect = Rect::new(x, y, w, h);
            para.render(rect, buf);
        }
        
        "ink-static" => {
            // Static renders its children as an overlay-like layer.
            // For parity we render children directly (reconciler handles semantics).
            for &child_id in &bridge::__ink_get_node_children(node_id).unwrap_or_default() {
                render_node(child_id, buf, area);
            }
        }
        
        "ink-newline" => {
            // Newline forces a blank line — render as empty Paragraph taking full width
            use ratatui::widgets::Paragraph;
            let rect = Rect::new(x, y, w.max(1), h.max(1));
            Paragraph::new("").render(rect, buf);
        }
        
        "ink-spacer" => {
            // Spacer is an invisible flex filler — nothing to render
        }
        
        _ => {}
    }
    
    // Render children (skip for static which already rendered them inline)
    if tag.as_str() != "ink-static" {
        if let Some(children) = bridge::__ink_get_node_children(node_id) {
            for &child_id in &children {
                render_node(child_id, buf, area);
            }
        }
    }
}

/// Parse color string to ratatui Color
fn parse_color(s: &str) -> Option<ratatui::style::Color> {
    match s.to_lowercase().as_str() {
        "black" => Some(ratatui::style::Color::Black),
        "red" => Some(ratatui::style::Color::Red),
        "green" => Some(ratatui::style::Color::Green),
        "yellow" => Some(ratatui::style::Color::Yellow),
        "blue" => Some(ratatui::style::Color::Blue),
        "magenta" => Some(ratatui::style::Color::Magenta),
        "cyan" => Some(ratatui::style::Color::Cyan),
        "white" => Some(ratatui::style::Color::White),
        "gray" | "grey" => Some(ratatui::style::Color::Gray),
        "brightblack" | "brightBlack" => Some(ratatui::style::Color::Indexed(8)),
        "brightred" | "brightRed" => Some(ratatui::style::Color::Indexed(9)),
        "brightgreen" | "brightGreen" => Some(ratatui::style::Color::Indexed(10)),
        "brightyellow" | "brightYellow" => Some(ratatui::style::Color::Indexed(11)),
        "brightblue" | "brightBlue" => Some(ratatui::style::Color::Indexed(12)),
        "brightmagenta" | "brightMagenta" => Some(ratatui::style::Color::Indexed(13)),
        "brightcyan" | "brightCyan" => Some(ratatui::style::Color::Indexed(14)),
        "brightwhite" | "brightWhite" => Some(ratatui::style::Color::Indexed(15)),
        _ => parse_hex_color(s),
    }
}

/// Parse hex color (#rgb or #rrggbb) to ratatui Color
fn parse_hex_color(s: &str) -> Option<ratatui::style::Color> {
    let s = s.trim();
    if !s.starts_with('#') {
        return None;
    }
    let hex = &s[1..];
    match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(ratatui::style::Color::Rgb(r, g, b))
        }
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            Some(ratatui::style::Color::Rgb(r, g, b))
        }
        _ => None,
    }
}

/// Convert crossterm KeyCode to Ink-compatible key name
/// This ensures useInput handlers receive the same key strings as Deno/Ink.
fn keycode_to_ink_name(key: &crossterm::event::KeyEvent) -> String {
    use crossterm::event::KeyCode;
    match key.code {
        KeyCode::Char(' ') => " ".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "return".to_string(),
        KeyCode::Esc => "escape".to_string(),
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Delete => "delete".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::Up => "upArrow".to_string(),
        KeyCode::Down => "downArrow".to_string(),
        KeyCode::Left => "leftArrow".to_string(),
        KeyCode::Right => "rightArrow".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::PageUp => "pageUp".to_string(),
        KeyCode::PageDown => "pageDown".to_string(),
        KeyCode::Insert => "insert".to_string(),
        KeyCode::BackTab => "tab".to_string(),
        KeyCode::F(n) => format!("f{}", n),
        _ => format!("{:?}", key.code).to_lowercase(),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    setup_logging();

    tracing::info!("TuiBridge v{} — Ink runtime in rquickjs + Rust", VERSION);

    // Parse command line args
    let args: Vec<String> = std::env::args().collect();
    
    if args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        println!("TuiBridge v{}", VERSION);
        println!();
        println!("Usage: tuibridge [OPTIONS] [SCRIPT]");
        println!();
        println!("Options:");
        println!("  --help, -h     Show this help");
        println!("  --version, -v  Show version");
        println!("  --bundle FILE  Load bundled JS from FILE");
        println!("  --eval CODE    Execute CODE");
        println!("  --watch PATH   Watch for file changes and hot reload");
        println!("  --hot          Enable hot reload mode (shortcut for --watch .)");
        println!("  --prop KEY=VAL Pass a prop to the JS runtime (useBridge().config)");
        println!();
        println!("Examples:");
        println!("  tuibridge --bundle plugins/app.tsx");
        println!("  tuibridge --hot examples/counter.js");
        println!("  tuibridge --watch plugins examples/app.js");
        return Ok(());
    }
    
    if args.contains(&"--version".to_string()) || args.contains(&"-v".to_string()) {
        println!("TuiBridge v{}", VERSION);
        return Ok(());
    }

    // Determine what to run and whether to enter interactive mode
    // Check for both --bundle and --eval (bundle is loaded first, then eval runs)
    let bundle_idx = args.iter().position(|a| a == "--bundle" || a == "-b");
    let eval_idx = args.iter().position(|a| a == "--eval" || a == "-e");
    let watch_idx = args.iter().position(|a| a == "--watch" || a == "-w");
    let hot_idx = args.iter().position(|a| a == "--hot");
    #[allow(unused_variables)]
    let watch_path = watch_idx.and_then(|i| args.get(i + 1).cloned())
        .or_else(|| hot_idx.map(|_| ".".to_string()));
    
    // Combine: bundle first, then eval (if both present)
    let (js_code, interactive) = match (bundle_idx, eval_idx) {
        (Some(bi), Some(ei)) => {
            let bundle = args.get(bi + 1)
                .and_then(|path| std::fs::read_to_string(path).ok())
                .unwrap_or_default();
            let eval = args.get(ei + 1).cloned().unwrap_or_default();
            (Some(format!("{}\n\n{}", bundle, eval)), true)
        }
        (Some(bi), None) => {
            let bundle = args.get(bi + 1)
                .and_then(|path| std::fs::read_to_string(path).ok());
            (bundle, true)
        }
        (None, Some(ei)) => {
            let eval = args.get(ei + 1).cloned();
            (eval, false)
        }
        (None, None) if args.len() > 1 && !args[1].starts_with('-') => {
            // Last argument is script file
            (std::fs::read_to_string(&args[args.len() - 1]).ok(), true)
        }
        _ => {
            // Default: show help, non-interactive
            (None, false)
        }
    };

    // Initialize QuickJS runtime
    tracing::debug!("Initializing QuickJS runtime");
    let runtime = rquickjs::Runtime::new()?;
    
    // Create context and setup bridge functions
    let ctx = rquickjs::Context::full(&runtime)?;
    
    // Register Ink API via native rquickjs bindings (Task 009b)
    ctx.with(|ctx| {
        if let Err(e) = ink_js::register(ctx) {
            tracing::warn!("ink_js::register error: {:?}", e);
        }
    });
    
    // Create a single Rust closure for __ink_call
    // This is the only closure that holds context references
    ctx.with(|ctx| {
        let globals = ctx.globals();
        
        let ink_call = rquickjs::Function::new(ctx.clone(), 
            |method: String, args_json: String| -> String {
                call_ink_ffi(&method, &args_json)
            }
        );
        
        globals.set("__ink_call", ink_call).ok();
    });
    
    // Load TuiBridge runtime (React reconciler + bridge wrappers)
    let runtime_js = include_str!("runtime.js");
    ctx.with(|ctx| {
        if let Err(e) = ctx.eval::<(), _>(runtime_js) {
            tracing::error!("Runtime load error: {:?}", e);
        } else {
            tracing::debug!("Runtime loaded successfully");
        }
    });
    
    // Inject bridge config (platform, terminal, user --prop flags)
    let bridge_config = bridge_config::BridgeConfig::from_args(&args);
    let config_js = bridge_config.to_js_injection();
    ctx.with(|ctx| {
        if let Err(e) = ctx.eval::<(), _>(config_js.as_str()) {
            tracing::warn!("Bridge config injection error: {:?}", e);
        } else {
            tracing::debug!("Bridge config injected");
        }
    });
    
    // Try to load precompiled bytecode bundle if available (production build)
    #[cfg(has_bytecode_bundle)]
    {
        // Note: QuickJS bytecode loading would use ctx.eval_bytes() or similar
        // For now, we just log that bytecode is available
        tracing::info!("Production build: bytecode bundle available");
        // In a real implementation with bundle_qbc module:
        // let bytes = bundle_qbc::BUNDLE_BYTECODE;
        // ctx.eval_bytes(bytes)?;
    }

    // Run the JS code and manage terminal
    // Note: JS code (via ink.render) will create its own root.
    // We track the actual root after JS runs.
    let mut root_id: Option<u32> = None;
    
    // If there's JS code, run it
    if let Some(ref code) = js_code {
        tracing::debug!("Executing user code");
        ctx.with(|ctx| {
            match ctx.eval::<(), _>(code.as_str()) {
                Ok(_) => tracing::debug!("Code executed successfully"),
                Err(e) => tracing::error!("Code execution error: {:?}", e),
            }
        });
        
        // Get the root created by JS (via ink.render or __ink_create_root)
        root_id = bridge::__ink_get_root_id();
        tracing::info!("Root node: {:?}", root_id);
    }
    
    // Create terminal
    tracing::debug!("Initializing terminal");
    
    // Check if stdout is a TTY
    let is_tty = atty::is(atty::Stream::Stdout);
    
    if !is_tty {
        tracing::info!("Not a TTY, skipping terminal initialization");
        tracing::info!("TuiBridge shutting down");
        std::process::exit(0);
    }
    
    // Try to enable raw mode, handle failure gracefully
    let raw_mode_result = crossterm::terminal::enable_raw_mode();
    if raw_mode_result.is_err() {
        tracing::warn!("Could not enable raw mode, skipping terminal initialization");
        tracing::info!("TuiBridge shutting down");
        std::process::exit(0);
    }
    
    let mut terminal = match Terminal::new(CrosstermBackend::new(stdout())) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("Could not create terminal: {:?}", e);
            let _ = crossterm::terminal::disable_raw_mode();
            tracing::info!("TuiBridge shutting down");
            std::process::exit(0);
        }
    };
    
    if let Err(e) = terminal.clear() {
        tracing::warn!("Could not clear terminal: {:?}", e);
    }
    
    // Hide cursor once for the entire session (prevents flicker on every draw)
    let _ = terminal.hide_cursor();
    
    // If not interactive, just run the initial render and exit
    if !interactive {
        tracing::info!("Non-interactive mode: rendering and exiting");
        
        // Do initial render
        if let Err(e) = render_tree(&mut terminal, root_id) {
            tracing::error!("Render error: {:?}", e);
        }
        
        // Cleanup terminal
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = terminal.show_cursor();
        
        tracing::info!("TuiBridge shutting down");
        
        // Force exit to bypass rquickjs GC assertion
        std::process::exit(0);
    }
    
    // Create event stream
    let mut event_stream = EventStream::new();
    
    // Setup hot reload if enabled
    #[cfg(feature = "hotreload")]
    let mut hot_reloader = if let Some(ref watch_path) = watch_path {
        match hotreload::HotReloader::new(watch_path) {
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
    
    // Keep track of the original script path for hot reload
    #[allow(unused_variables)]
    let script_path = bundle_idx.and_then(|i| args.get(i + 1).cloned())
        .or_else(|| args.get(1).cloned().filter(|s| !s.starts_with('-')));
    
    // Run the event loop
    tracing::info!("Starting event loop");
    
    let mut dirty = true;
    
    loop {
        // Check for exit
        if bridge::__ink_should_exit() {
            tracing::info!("Exit requested, code: {}", bridge::__ink_get_exit_code());
            break;
        }
        
        tokio::select! {
            // Handle terminal events
            evt = event_stream.next() => {
                match evt {
                    Some(Ok(Event::Key(key))) => {
                        let is_backtab = matches!(key.code, crossterm::event::KeyCode::BackTab);
                        let key_str = keycode_to_ink_name(&key);
                        let ctrl = key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL);
                        let shift = key.modifiers.contains(crossterm::event::KeyModifiers::SHIFT) || is_backtab;
                        let alt = key.modifiers.contains(crossterm::event::KeyModifiers::ALT);
                        let meta = key.modifiers.contains(crossterm::event::KeyModifiers::META)
                            || key.modifiers.contains(crossterm::event::KeyModifiers::SUPER);

                        // Dispatch to JS runtime handlers
                        ctx.with(|ctx| {
                            let code = format!(
                                "try {{ if (globalThis.__tb_dispatch_key) __tb_dispatch_key('{}', {}, {}, {}, {}); }} catch(e) {{ console.error(e); }}",
                                key_str.replace("'", "\\'"),
                                ctrl, shift, alt, meta
                            );
                            if let Err(e) = ctx.eval::<(), _>(&*code) {
                                tracing::warn!("Key dispatch error: {:?}", e);
                            }
                        });
                        
                        // Check if JS rendered anything
                        dirty = dirty || bridge::__ink_is_dirty();
                    }
                    Some(Ok(Event::Mouse(mouse))) => {
                        use crossterm::event::MouseEventKind;
                        
                        let kind_str = match mouse.kind {
                            MouseEventKind::Down(_) => "press",
                            MouseEventKind::Up(_) => "release",
                            MouseEventKind::Drag(_) | MouseEventKind::Moved => "hold",
                            MouseEventKind::ScrollUp => "wheelUp",
                            MouseEventKind::ScrollDown => "wheelDown",
                            _ => "unknown",
                        };
                        
                        let shift = mouse.modifiers.contains(crossterm::event::KeyModifiers::SHIFT);
                        let ctrl = mouse.modifiers.contains(crossterm::event::KeyModifiers::CONTROL);
                        let alt = mouse.modifiers.contains(crossterm::event::KeyModifiers::ALT);
                        
                        tracing::debug!("Mouse event: {} at ({}, {})", kind_str, mouse.column, mouse.row);
                        
                        // Dispatch to JS runtime handlers
                        ctx.with(|ctx| {
                            let code = format!(
                                "try {{ if (globalThis.__tb_dispatch_mouse) __tb_dispatch_mouse({{kind:'{}',column:{},row:{},shift:{},ctrl:{},alt:{}}}); }} catch(e) {{ console.error(e); }}",
                                kind_str, mouse.column, mouse.row, shift, ctrl, alt
                            );
                            if let Err(e) = ctx.eval::<(), _>(&*code) {
                                tracing::warn!("Mouse dispatch error: {:?}", e);
                            }
                        });
                        
                        dirty = dirty || bridge::__ink_is_dirty();
                    }
                    Some(Ok(Event::Resize(cols, rows))) => {
                        tracing::debug!("Terminal resize: {}x{}", cols, rows);
                        bridge::__ink_set_terminal_size(cols as u32, rows as u32);
                        dirty = true;
                    }
                    _ => {}
                }
            }
            
            // Handle timer callbacks (polled)
            _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
                // OPTIMIZED: Process microtasks and timers via single JS call
                // Avoids per-callback eval() - 10x speedup for hot path
                ctx.with(|ctx| {
                    // First, invoke microtasks (drained in JS via __tb_invoke_microtasks)
                    if bridge::__ink_drain_microtasks() {
                        let microtask_code = "try { if (globalThis.__tb_invoke_microtasks) __tb_invoke_microtasks(); } catch(e) { console.error(e); }";
                        if let Err(e) = ctx.eval::<(), _>(microtask_code) {
                            tracing::warn!("Microtask dispatch error: {:?}", e);
                        }
                    }
                    
                    // Then, invoke timers (via __tb_invoke_timers)
                    let timer_ids = bridge::__ink_process_timers();
                    if timer_ids != "[]" {
                        let timer_code = format!(
                            "try {{ if (globalThis.__tb_invoke_timers) __tb_invoke_timers({}); }} catch(e) {{ console.error(e); }}",
                            timer_ids
                        );
                        if let Err(e) = ctx.eval::<(), _>(&*timer_code) {
                            tracing::warn!("Timer dispatch error: {:?}", e);
                        }
                    }
                });
                
                dirty = dirty || bridge::__ink_is_dirty();
            }
        }
        
        // Handle hot reload (outside select to avoid cfg issues)
        #[cfg(feature = "hotreload")]
        if let Some(ref mut reloader) = hot_reloader {
            if let Some(_event) = reloader.poll_changes() {
                tracing::info!("Hot reload: File changed, reloading...");
                
                // Unmount old app
                if let Some(old_root_id) = bridge::__ink_get_root_id() {
                    bridge::__ink_destroy_root(old_root_id);
                }
                
                // Reload and re-execute the script
                if let Some(ref path) = script_path {
                    if let Ok(new_code) = std::fs::read_to_string(path) {
                        let start = std::time::Instant::now();
                        ctx.with(|ctx| {
                            match ctx.eval::<(), _>(new_code.as_str()) {
                                Ok(_) => {
                                    let elapsed = start.elapsed();
                                    tracing::info!("Hot reload complete in {:?}", elapsed);
                                    if elapsed.as_millis() < 50 {
                                        tracing::debug!("Hot reload under 50ms target");
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("Hot reload eval error: {:?}", e);
                                }
                            }
                        });
                        
                        // Get new root
                        root_id = bridge::__ink_get_root_id();
                        dirty = true;
                    } else {
                        tracing::error!("Hot reload: Failed to read {}", path);
                    }
                }
            }
        }
        
        // Render if dirty
        if dirty {
            if let Err(e) = render_tree(&mut terminal, root_id) {
                tracing::error!("Render error: {:?}", e);
            }
            bridge::__ink_clear_dirty();
            dirty = false;
        }
    }
    
    // Cleanup terminal
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = terminal.show_cursor();
    
    tracing::info!("TuiBridge shutting down");
    
    // Force exit to bypass rquickjs GC assertion
    std::process::exit(0);
}

/// Helper function to call bridge from JavaScript
/// args_json is a JSON array string containing the arguments
fn call_ink_ffi(method: &str, args_json: &str) -> String {
    // Parse the JSON args - handle both string and number arrays
    let args: Vec<serde_json::Value> = serde_json::from_str(args_json).unwrap_or_default();
    // Convert to strings for convenience
    let args: Vec<String> = args.iter().map(|v| match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        _ => v.to_string(),
    }).collect();
    
    match method {
        "create_root" => (bridge::__ink_create_root() as f64).to_string(),
        "render_element" => {
            let json = args.first().cloned().unwrap_or_default();
            (bridge::__ink_render_element(&json) as f64).to_string()
        }
        "destroy_root" => { 
            let id = args.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            bridge::__ink_destroy_root(id);
            String::new()
        }
        "create_node" => {
            let tag = args.first().cloned().unwrap_or_default();
            let props = args.get(1).cloned().unwrap_or_default();
            (bridge::__ink_create_node(&tag, &props).unwrap_or(0) as f64).to_string()
        }
        "create_text_node" => {
            let text = args.first().cloned().unwrap_or_default();
            (bridge::__ink_create_text_node(&text) as f64).to_string()
        }
        "append_child" => {
            let p = args.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            let c = args.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            (bridge::__ink_append_child(p, c).is_ok()).to_string()
        }
        "remove_child" => {
            let p = args.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            let c = args.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            (bridge::__ink_remove_child(p, c).is_ok()).to_string()
        }
        "insert_before" => {
            let p = args.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            let c = args.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            let b = args.get(2).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            (bridge::__ink_insert_before(p, c, b).is_ok()).to_string()
        }
        "commit_update" => {
            let id = args.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            let props = args.get(1).cloned().unwrap_or_default();
            (bridge::__ink_commit_update(id, &props).is_ok()).to_string()
        }
        "set_text" => {
            let id = args.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            let text = args.get(1).cloned().unwrap_or_default();
            (bridge::__ink_set_text(id, &text).is_ok()).to_string()
        }
        "commit" => {
            bridge::__ink_commit();
            String::new()
        }
        "is_dirty" => bridge::__ink_is_dirty().to_string(),
        "clear_dirty" => { bridge::__ink_clear_dirty(); String::new() }
        "measure_text" => {
            let text = args.first().cloned().unwrap_or_default();
            let width = args.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(80);
            let (w, h) = bridge::__ink_measure_text(&text, width);
            format!("{},{}" , w, h)
        }
        "measure_element" => {
            let id = args.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            match bridge::__ink_measure_element(id) {
                Some((w, h)) => format!("{},{}" , w, h),
                None => "null".to_string(),
            }
        }
        "exit" => {
            let code = args.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as i32;
            bridge::__ink_exit(code);
            String::new()
        }
        "should_exit" => bridge::__ink_should_exit().to_string(),
        "get_exit_code" => (bridge::__ink_get_exit_code() as f64).to_string(),
        "reset_exit" => { bridge::__ink_reset_exit(); String::new() }
        "set_terminal_size" => {
            let w = args.first().and_then(|s| s.parse::<u32>().ok()).unwrap_or(80);
            let h = args.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(24);
            bridge::__ink_set_terminal_size(w, h);
            String::new()
        }
        "get_terminal_size" => {
            let (w, h) = bridge::__ink_get_terminal_size();
            format!("{},{}" , w, h)
        }
        "get_node_tag" => {
            let id = args.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            bridge::__ink_get_node_tag(id).unwrap_or_else(|| "null".to_string())
        }
        "get_node_text" => {
            let id = args.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            bridge::__ink_get_node_text(id).unwrap_or_else(|| "null".to_string())
        }
        "get_node_children" => {
            let id = args.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            match bridge::__ink_get_node_children(id) {
                Some(children) => {
                    let s: Vec<String> = children.iter().map(|&c| c.to_string()).collect();
                    format!("[{}]", s.join(","))
                }
                None => "null".to_string(),
            }
        }
        "get_node_parent" => {
            let id = args.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            match bridge::__ink_get_node_parent(id) {
                Some(parent_id) => parent_id.to_string(),
                None => "null".to_string(),
            }
        }
        "get_node_prop" => {
            let id = args.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            let prop = args.get(1).cloned().unwrap_or_default();
            bridge::__ink_get_node_prop(id, &prop).unwrap_or_else(|| "null".to_string())
        }
        "get_root_id" => {
            match bridge::__ink_get_root_id() {
                Some(id) => id.to_string(),
                None => "null".to_string(),
            }
        }
        "calculate_layout" => (bridge::__ink_calculate_layout().is_ok()).to_string(),
        "get_layout" => {
            let id = args.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            match bridge::__ink_get_layout(id) {
                Some((x, y, w, h)) => format!("{},{},{},{}" , x, y, w, h),
                None => "null".to_string(),
            }
        }
        "stdout_write" => {
            let data = args.first().cloned().unwrap_or_default();
            bridge::__ink_stdout_write(&data);
            String::new()
        }
        "stderr_write" => {
            let data = args.first().cloned().unwrap_or_default();
            bridge::__ink_stderr_write(&data);
            String::new()
        }
        "stdin_is_raw" => bridge::__ink_stdin_is_raw().to_string(),
        "set_raw_mode" => {
            let enabled = args.first().cloned().unwrap_or_default() == "true";
            if enabled {
                let _ = crossterm::terminal::enable_raw_mode();
            } else {
                let _ = crossterm::terminal::disable_raw_mode();
            }
            String::new()
        }
        // Timer polyfills
        "set_timeout" => {
            let callback = args.first().cloned().unwrap_or_default();
            let delay = args.get(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            (bridge::__ink_set_timeout(&callback, delay) as f64).to_string()
        }
        "set_interval" => {
            let callback = args.first().cloned().unwrap_or_default();
            let interval = args.get(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            (bridge::__ink_set_interval(&callback, interval) as f64).to_string()
        }
        "clear_timer" => {
            let id = args.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            (bridge::__ink_clear_timer(id)).to_string()
        }
        "process_timers" => {
            // Returns JSON array of timer IDs (e.g., "[1,2,3]")
            bridge::__ink_process_timers()
        }
        "has_pending_timers" => bridge::__ink_has_pending_timers().to_string(),
        "next_timer_delay" => {
            match bridge::__ink_next_timer_delay() {
                Some(d) => d.as_millis().to_string(),
                None => "-1".to_string(),
            }
        }
        // Microtask polyfills
        "enqueue_microtask" => {
            let callback = args.first().cloned().unwrap_or_default();
            bridge::__ink_enqueue_microtask(&callback);
            String::new()
        }
        "drain_microtasks" => {
            // Returns bool - microtask execution is handled by __tb_invoke_microtasks in JS
            bridge::__ink_drain_microtasks().to_string()
        }
        _ => String::new(),
    }
}
