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
    
    let tag = match bridge::__ink_get_node_tag(node_id) {
        Some(t) => t,
        None => return,
    };
    
    let layout = match bridge::__ink_get_layout(node_id) {
        Some(l) => l,
        None => return,
    };
    
    let x = layout.0 as u16;
    let y = layout.1 as u16;
    let w = layout.2 as u16;
    let h = layout.3 as u16;
    
    // Skip if out of bounds
    if x >= area.right() || y >= area.bottom() {
        return;
    }
    
    match tag.as_str() {
        "ink-box" => {
            use ratatui::widgets::Block;
            use ratatui::widgets::Borders;
            
            // Check for border style
            let border_style = bridge::__ink_get_node_prop(node_id, "borderStyle")
                .map(|s| s.trim_matches('"').to_string());
            
            let mut block = Block::default();
            
            if let Some(ref style) = border_style {
                match style.as_str() {
                    "round" | "bold" | "single" | "double" => {
                        block = block.borders(Borders::ALL);
                    }
                    _ => {}
                }
            }
            
            // Add title if present
            if let Some(title) = bridge::__ink_get_node_prop(node_id, "title")
                .map(|s| s.trim_matches('"').to_string()) {
                block = block.title(title);
            }
            
            let rect = Rect::new(x, y, w, h);
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
            
            let para = Paragraph::new(text.as_str())
                .style(style);
            
            let rect = Rect::new(x, y, w, h);
            para.render(rect, buf);
        }
        
        _ => {}
    }
    
    // Render children
    if let Some(children) = bridge::__ink_get_node_children(node_id) {
        for &child_id in &children {
            render_node(child_id, buf, area);
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
        _ => None,
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
        println!();
        println!("Examples:");
        println!("  tuibridge --bundle plugins/app.tsx");
        println!("  tuibridge --eval 'console.log(\"Hello, TuiBridge!\")'");
        return Ok(());
    }
    
    if args.contains(&"--version".to_string()) || args.contains(&"-v".to_string()) {
        println!("TuiBridge v{}", VERSION);
        return Ok(());
    }

    // Determine what to run and whether to enter interactive mode
    let (js_code, interactive) = if let Some(idx) = args.iter().position(|a| a == "--eval" || a == "-e") {
        (args.get(idx + 1).cloned(), false)
    } else if let Some(idx) = args.iter().position(|a| a == "--bundle" || a == "-b") {
        (args.get(idx + 1).and_then(|path| std::fs::read_to_string(path).ok()), true)
    } else if args.len() > 1 && !args[1].starts_with('-') {
        // Last argument is script file
        (std::fs::read_to_string(&args[args.len() - 1]).ok(), true)
    } else {
        // Default: show help, non-interactive
        (None, false)
    };

    // Initialize QuickJS runtime
    tracing::debug!("Initializing QuickJS runtime");
    let runtime = rquickjs::Runtime::new()?;
    
    // Create context and setup bridge functions
    let ctx = rquickjs::Context::full(&runtime)?;
    
    // Setup globals using eval strings to avoid closure reference cycles
    ctx.with(|ctx| {
        // Define all __ink_* functions using eval to avoid closure reference issues
        // All functions use JSON for parameter passing to simplify Rust side
        let init_code = r#"
        // Global state for tracking
        globalThis.__ink_callbacks = {};
        
        // __ink_call - bridge dispatcher using JSON args
        // Expected format: __ink_call(method, argsJson)
        // Returns result as JSON string
        globalThis.__ink_call = function(method, argsJson) {
            var args = argsJson ? JSON.parse(argsJson) : [];
            switch(method) {
                case 'create_root':
                    return String(__ink_create_root());
                case 'destroy_root':
                    __ink_destroy_root(args[0]);
                    return '';
                case 'create_node':
                    return String(__ink_create_node(args[0], args[1]));
                case 'create_text_node':
                    return String(__ink_create_text_node(args[0]));
                case 'append_child':
                    return String(__ink_append_child(args[0], args[1]));
                case 'remove_child':
                    return String(__ink_remove_child(args[0], args[1]));
                case 'insert_before':
                    return String(__ink_insert_before(args[0], args[1], args[2]));
                case 'commit_update':
                    return String(__ink_commit_update(args[0], args[1]));
                case 'set_text':
                    return String(__ink_set_text(args[0], args[1]));
                case 'commit':
                    __ink_commit();
                    return '';
                case 'is_dirty':
                    return __ink_is_dirty() ? 'true' : 'false';
                case 'clear_dirty':
                    __ink_clear_dirty();
                    return '';
                case 'measure_text':
                    return __ink_measure_text(args[0], args[1]);
                case 'measure_element':
                    return __ink_measure_element(args[0]);
                case 'exit':
                    __ink_exit(args[0]);
                    return '';
                case 'should_exit':
                    return __ink_should_exit() ? 'true' : 'false';
                case 'get_exit_code':
                    return String(__ink_get_exit_code());
                case 'reset_exit':
                    __ink_reset_exit();
                    return '';
                case 'set_terminal_size':
                    __ink_set_terminal_size(args[0], args[1]);
                    return '';
                case 'get_terminal_size':
                    return __ink_get_terminal_size();
                case 'get_node_tag':
                    return __ink_get_node_tag(args[0]);
                case 'get_node_text':
                    return __ink_get_node_text(args[0]);
                case 'get_node_children':
                    return __ink_get_node_children(args[0]);
                case 'get_node_prop':
                    return __ink_get_node_prop(args[0], args[1]);
                case 'get_root_id':
                    return __ink_get_root_id();
                case 'calculate_layout':
                    return String(__ink_calculate_layout());
                case 'get_layout':
                    return __ink_get_layout(args[0]);
                case 'register_input':
                    return String(__ink_register_input(args[0]));
                case 'unregister_input':
                    __ink_unregister_input(args[0]);
                    return '';
                case 'stdout_write':
                    __ink_stdout_write(args[0]);
                    return '';
                case 'stderr_write':
                    __ink_stderr_write(args[0]);
                    return '';
                case 'stdin_is_raw':
                    return __ink_stdin_is_raw() ? 'true' : 'false';
                case 'set_raw_mode':
                    __ink_set_raw_mode(args[0]);
                    return '';
                default:
                    return '';
            }
        };
        
        // Helper to call bridge with arguments as array
        globalThis.__ink_ffi = function(method) {
            var args = Array.prototype.slice.call(arguments, 1);
            return JSON.parse(__ink_call(method, JSON.stringify(args)));
        };
        
        // Simplified wrappers for common operations
        globalThis.__ink_create_root = function() {
            var id = parseFloat(__ink_call('create_root', '[]'));
            return id;
        };
        
        globalThis.__ink_destroy_root = function(id) {
            __ink_call('destroy_root', JSON.stringify([id]));
        };
        
        globalThis.__ink_create_node = function(tag, props) {
            var id = parseFloat(__ink_call('create_node', JSON.stringify([tag, props])));
            return id;
        };
        
        globalThis.__ink_create_text_node = function(text) {
            var id = parseFloat(__ink_call('create_text_node', JSON.stringify([text])));
            return id;
        };
        
        globalThis.__ink_append_child = function(parent, child) {
            return __ink_call('append_child', JSON.stringify([parent, child])) === 'true';
        };
        
        globalThis.__ink_remove_child = function(parent, child) {
            return __ink_call('remove_child', JSON.stringify([parent, child])) === 'true';
        };
        
        globalThis.__ink_insert_before = function(parent, child, before) {
            return __ink_call('insert_before', JSON.stringify([parent, child, before])) === 'true';
        };
        
        globalThis.__ink_commit_update = function(id, props) {
            return __ink_call('commit_update', JSON.stringify([id, props])) === 'true';
        };
        
        globalThis.__ink_set_text = function(id, text) {
            return __ink_call('set_text', JSON.stringify([id, text])) === 'true';
        };
        
        globalThis.__ink_commit = function() {
            __ink_call('commit', '[]');
        };
        
        globalThis.__ink_is_dirty = function() {
            return __ink_call('is_dirty', '[]') === 'true';
        };
        
        globalThis.__ink_clear_dirty = function() {
            __ink_call('clear_dirty', '[]');
        };
        
        globalThis.__ink_measure_text = function(text, width) {
            var result = __ink_call('measure_text', JSON.stringify([text, width]));
            var parts = result.split(',');
            return { width: parseInt(parts[0]) || 0, height: parseInt(parts[1]) || 0 };
        };
        
        globalThis.__ink_measure_element = function(id) {
            var result = __ink_call('measure_element', JSON.stringify([id]));
            if (result === 'null') return null;
            var parts = result.split(',');
            return { width: parseFloat(parts[0]) || 0, height: parseFloat(parts[1]) || 0 };
        };
        
        globalThis.__ink_exit = function(code) {
            __ink_call('exit', JSON.stringify([code || 0]));
        };
        
        globalThis.__ink_should_exit = function() {
            return __ink_call('should_exit', '[]') === 'true';
        };
        
        globalThis.__ink_get_exit_code = function() {
            return parseFloat(__ink_call('get_exit_code', '[]')) || 0;
        };
        
        globalThis.__ink_reset_exit = function() {
            __ink_call('reset_exit', '[]');
        };
        
        globalThis.__ink_set_terminal_size = function(width, height) {
            __ink_call('set_terminal_size', JSON.stringify([width, height]));
        };
        
        globalThis.__ink_get_terminal_size = function() {
            var result = __ink_call('get_terminal_size', '[]');
            var parts = result.split(',');
            return { width: parseInt(parts[0]) || 0, height: parseInt(parts[1]) || 0 };
        };
        
        globalThis.__ink_get_node_tag = function(id) {
            var result = __ink_call('get_node_tag', JSON.stringify([id]));
            return result === 'null' ? null : result;
        };
        
        globalThis.__ink_get_node_text = function(id) {
            var result = __ink_call('get_node_text', JSON.stringify([id]));
            return result === 'null' ? null : result;
        };
        
        globalThis.__ink_get_node_children = function(id) {
            var result = __ink_call('get_node_children', JSON.stringify([id]));
            if (result === 'null') return null;
            try {
                return JSON.parse(result);
            } catch(e) {
                return null;
            }
        };
        
        globalThis.__ink_get_node_prop = function(id, prop) {
            var result = __ink_call('get_node_prop', JSON.stringify([id, prop]));
            return result === 'null' ? null : result;
        };
        
        globalThis.__ink_get_root_id = function() {
            var result = __ink_call('get_root_id', '[]');
            return result === 'null' ? null : parseFloat(result) || null;
        };
        
        globalThis.__ink_calculate_layout = function() {
            return __ink_call('calculate_layout', '[]') === 'true';
        };
        
        globalThis.__ink_get_layout = function(id) {
            var result = __ink_call('get_layout', JSON.stringify([id]));
            if (result === 'null') return null;
            var parts = result.split(',');
            return {
                left: parseFloat(parts[0]) || 0,
                top: parseFloat(parts[1]) || 0,
                width: parseFloat(parts[2]) || 0,
                height: parseFloat(parts[3]) || 0
            };
        };
        
        globalThis.__ink_register_input = function(callback) {
            return parseFloat(__ink_call('register_input', JSON.stringify([callback])));
        };
        
        globalThis.__ink_unregister_input = function(id) {
            __ink_call('unregister_input', JSON.stringify([id]));
        };
        
        globalThis.__ink_stdout_write = function(data) {
            __ink_call('stdout_write', JSON.stringify([data]));
        };
        
        globalThis.__ink_stderr_write = function(data) {
            __ink_call('stderr_write', JSON.stringify([data]));
        };
        
        globalThis.__ink_stdin_is_raw = function() {
            return __ink_call('stdin_is_raw', '[]') === 'true';
        };
        
        globalThis.__ink_set_raw_mode = function(enabled) {
            __ink_call('set_raw_mode', JSON.stringify([enabled]));
        };
        
        // Console polyfill using __ink_stdout_write
        globalThis.console = {
            log: function() { 
                var args = Array.prototype.slice.call(arguments);
                var msg = args.map(function(v) { return String(v); }).join(' ') + '\n';
                try { __ink_stdout_write(msg); } catch(e) {}
            },
            error: function() { 
                var args = Array.prototype.slice.call(arguments);
                var msg = '[ERROR] ' + args.map(function(v) { return String(v); }).join(' ') + '\n';
                try { __ink_stderr_write(msg); } catch(e) {}
            },
            warn: function() { 
                var args = Array.prototype.slice.call(arguments);
                var msg = '[WARN] ' + args.map(function(v) { return String(v); }).join(' ') + '\n';
                try { __ink_stdout_write(msg); } catch(e) {}
            },
            info: function() { 
                var args = Array.prototype.slice.call(arguments);
                var msg = '[INFO] ' + args.map(function(v) { return String(v); }).join(' ') + '\n';
                try { __ink_stdout_write(msg); } catch(e) {}
            }
        };
        
        // Process polyfill
        globalThis.process = {
            stdout: {
                write: function(s) { try { __ink_stdout_write(String(s)); } catch(e) {} }
            },
            stderr: {
                write: function(s) { try { __ink_stderr_write('[STDERR] ' + String(s)); } catch(e) {} }
            }
        };
        "#;
        
        ctx.eval::<(), _>(init_code).ok();
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
    
    // Run the JS code and manage terminal
    let root_id = bridge::__ink_create_root();
    tracing::info!("Created root node: {}", root_id);
    
    // If there's JS code, run it
    if let Some(ref code) = js_code {
        tracing::debug!("Executing user code");
        ctx.with(|ctx| {
            match ctx.eval::<(), _>(code.as_str()) {
                Ok(_) => tracing::debug!("Code executed successfully"),
                Err(e) => tracing::error!("Code execution error: {:?}", e),
            }
        });
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
    
    // If not interactive, just run the initial render and exit
    if !interactive {
        tracing::info!("Non-interactive mode: rendering and exiting");
        
        // Do initial render
        if let Err(e) = render_tree(&mut terminal, Some(root_id)) {
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
                        let key_str = format!("{:?}", key.code);
                        let ctrl = key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL);
                        let shift = key.modifiers.contains(crossterm::event::KeyModifiers::SHIFT);
                        let alt = key.modifiers.contains(crossterm::event::KeyModifiers::ALT);
                        
                        // Dispatch to JS
                        let callback_js = bridge::__ink_dispatch_key(&key_str, ctrl, shift, alt);
                        if !callback_js.is_empty() && callback_js != "[]" {
                            ctx.with(|ctx| {
                                let code = format!("try {{ {} }} catch(e) {{ console.error(e) }}", callback_js);
                                if let Err(e) = ctx.eval::<(), _>(&*code) {
                                    tracing::warn!("Callback error: {:?}", e);
                                }
                            });
                        }
                        
                        // Check if JS rendered anything
                        dirty = dirty || bridge::__ink_is_dirty();
                    }
                    Some(Ok(Event::Mouse(_))) => {
                        // TODO: Handle mouse events
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
                // Process timers - handled by JS
                dirty = dirty || bridge::__ink_is_dirty();
            }
        }
        
        // Render if dirty
        if dirty {
            if let Err(e) = render_tree(&mut terminal, Some(root_id)) {
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
    // Parse the JSON args
    let args: Vec<String> = serde_json::from_str(args_json).unwrap_or_default();
    
    match method {
        "create_root" => (bridge::__ink_create_root() as f64).to_string(),
        "destroy_root" => { 
            let id = args.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            bridge::__ink_destroy_root(id);
            String::new()
        }
        "create_node" => {
            let tag = args.get(0).cloned().unwrap_or_default();
            let props = args.get(1).cloned().unwrap_or_default();
            (bridge::__ink_create_node(&tag, &props).unwrap_or(0) as f64).to_string()
        }
        "create_text_node" => {
            let text = args.first().cloned().unwrap_or_default();
            (bridge::__ink_create_text_node(&text) as f64).to_string()
        }
        "append_child" => {
            let p = args.get(0).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            let c = args.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            (bridge::__ink_append_child(p, c).is_ok()).to_string()
        }
        "remove_child" => {
            let p = args.get(0).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            let c = args.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            (bridge::__ink_remove_child(p, c).is_ok()).to_string()
        }
        "insert_before" => {
            let p = args.get(0).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            let c = args.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            let b = args.get(2).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            (bridge::__ink_insert_before(p, c, b).is_ok()).to_string()
        }
        "commit_update" => {
            let id = args.get(0).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            let props = args.get(1).cloned().unwrap_or_default();
            (bridge::__ink_commit_update(id, &props).is_ok()).to_string()
        }
        "set_text" => {
            let id = args.get(0).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
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
            let text = args.get(0).cloned().unwrap_or_default();
            let width = args.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(80);
            let (w, h) = bridge::__ink_measure_text(&text, width);
            format!("{},{}", w, h)
        }
        "measure_element" => {
            let id = args.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            match bridge::__ink_measure_element(id) {
                Some((w, h)) => format!("{},{}", w, h),
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
            let w = args.get(0).and_then(|s| s.parse::<u32>().ok()).unwrap_or(80);
            let h = args.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(24);
            bridge::__ink_set_terminal_size(w, h);
            String::new()
        }
        "get_terminal_size" => {
            let (w, h) = bridge::__ink_get_terminal_size();
            format!("{},{}", w, h)
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
        "get_node_prop" => {
            let id = args.get(0).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
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
                Some((x, y, w, h)) => format!("{},{},{},{}", x, y, w, h),
                None => "null".to_string(),
            }
        }
        "register_input" => {
            let callback = args.first().cloned().unwrap_or_default();
            (bridge::__ink_register_input(&callback) as f64).to_string()
        }
        "unregister_input" => {
            let id = args.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u32;
            bridge::__ink_unregister_input(id);
            String::new()
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
        _ => String::new(),
    }
}
