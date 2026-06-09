//! TuiBridge — Run Ink (React for terminals) using rquickjs + Rust
//!
//! This binary loads a bundled JavaScript application (Ink-compatible)
//! and executes it in a QuickJS runtime with Yoga layout and ratatui rendering.

#![deny(unused_must_use)]
#![deny(clippy::all)]
#![allow(clippy::upper_case_acronyms)]
#![allow(dead_code)]

mod ink;
mod ffi;

use anyhow::Result;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io::{stdout, Write};

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
        ffi::__ink_set_terminal_size(area.width as u32, area.height as u32);
        
        // Calculate layout
        if let Err(e) = ffi::__ink_calculate_layout() {
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
    
    let tag = match ffi::__ink_get_node_tag(node_id) {
        Some(t) => t,
        None => return,
    };
    
    let layout = match ffi::__ink_get_layout(node_id) {
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
            let border_style = ffi::__ink_get_node_prop(node_id, "borderStyle")
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
            if let Some(title) = ffi::__ink_get_node_prop(node_id, "title")
                .map(|s| s.trim_matches('"').to_string()) {
                block = block.title(title);
            }
            
            let rect = Rect::new(x, y, w, h);
            block.render(rect, buf);
        }
        
        "ink-text" => {
            use ratatui::widgets::Paragraph;
            
            let text = ffi::__ink_get_node_text(node_id).unwrap_or_default();
            
            // Check for color prop
            let mut style = Style::default();
            if let Some(color) = ffi::__ink_get_node_prop(node_id, "color")
                .map(|s| s.trim_matches('"').to_string()) {
                if let Some(c) = parse_color(&color) {
                    style = style.fg(c);
                }
            }
            
            // Check for bold
            if ffi::__ink_get_node_prop(node_id, "bold").is_some() {
                style = style.add_modifier(ratatui::style::Modifier::BOLD);
            }
            
            // Check for dim
            if ffi::__ink_get_node_prop(node_id, "dimColor").is_some() {
                style = style.add_modifier(ratatui::style::Modifier::DIM);
            }
            
            // Check for italic
            if ffi::__ink_get_node_prop(node_id, "italic").is_some() {
                style = style.add_modifier(ratatui::style::Modifier::ITALIC);
            }
            
            // Check for strikethrough
            if ffi::__ink_get_node_prop(node_id, "strikethrough").is_some() {
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
    if let Some(children) = ffi::__ink_get_node_children(node_id) {
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

    // Initialize QuickJS runtime
    tracing::debug!("Initializing QuickJS runtime");
    let runtime = rquickjs::Runtime::new()?;
    
    // Create context and setup FFI
    let ctx = rquickjs::Context::full(&runtime)?;
    
    ctx.with(|ctx| {
        // Setup global functions
        let globals = ctx.globals();
        
        // Helper to create a JS function that calls a Rust function
        // We'll use eval strings for simplicity
        let ctx2 = ctx.clone();
        let eval_js = rquickjs::Function::new(ctx.clone(), move |code: String| {
            let _: Result<(), _> = ctx2.eval(&*code);
        });
        globals.set("__ink_eval", eval_js)?;
        
        // __ink_create_root
        let create_root = rquickjs::Function::new(ctx.clone(), || {
            (ffi::__ink_create_root() as f64).to_string()
        });
        globals.set("__ink_create_root", create_root)?;
        
        // __ink_destroy_root  
        let destroy_root = rquickjs::Function::new(ctx.clone(), |id: String| {
            let id = id.parse::<f64>().unwrap_or(0.0) as u32;
            ffi::__ink_destroy_root(id);
        });
        globals.set("__ink_destroy_root", destroy_root)?;
        
        // __ink_create_node
        let create_node = rquickjs::Function::new(ctx.clone(), |tag: String, props: String| {
            (ffi::__ink_create_node(&tag, &props).unwrap_or(0) as f64).to_string()
        });
        globals.set("__ink_create_node", create_node)?;
        
        // __ink_create_text_node
        let create_text_node = rquickjs::Function::new(ctx.clone(), |text: String| {
            (ffi::__ink_create_text_node(&text) as f64).to_string()
        });
        globals.set("__ink_create_text_node", create_text_node)?;
        
        // __ink_append_child
        let append_child = rquickjs::Function::new(ctx.clone(), |parent: String, child: String| {
            let p = parent.parse::<f64>().unwrap_or(0.0) as u32;
            let c = child.parse::<f64>().unwrap_or(0.0) as u32;
            ffi::__ink_append_child(p, c).is_ok()
        });
        globals.set("__ink_append_child", append_child)?;
        
        // __ink_remove_child
        let remove_child = rquickjs::Function::new(ctx.clone(), |parent: String, child: String| {
            let p = parent.parse::<f64>().unwrap_or(0.0) as u32;
            let c = child.parse::<f64>().unwrap_or(0.0) as u32;
            ffi::__ink_remove_child(p, c).is_ok()
        });
        globals.set("__ink_remove_child", remove_child)?;
        
        // __ink_insert_before
        let insert_before = rquickjs::Function::new(ctx.clone(), |parent: String, child: String, before: String| {
            let p = parent.parse::<f64>().unwrap_or(0.0) as u32;
            let c = child.parse::<f64>().unwrap_or(0.0) as u32;
            let b = before.parse::<f64>().unwrap_or(0.0) as u32;
            ffi::__ink_insert_before(p, c, b).is_ok()
        });
        globals.set("__ink_insert_before", insert_before)?;
        
        // __ink_commit_update
        let commit_update = rquickjs::Function::new(ctx.clone(), |id: String, props: String| {
            let id = id.parse::<f64>().unwrap_or(0.0) as u32;
            ffi::__ink_commit_update(id, &props).is_ok()
        });
        globals.set("__ink_commit_update", commit_update)?;
        
        // __ink_set_text
        let set_text = rquickjs::Function::new(ctx.clone(), |id: String, text: String| {
            let id = id.parse::<f64>().unwrap_or(0.0) as u32;
            ffi::__ink_set_text(id, &text).is_ok()
        });
        globals.set("__ink_set_text", set_text)?;
        
        // __ink_commit
        let commit = rquickjs::Function::new(ctx.clone(), || {
            ffi::__ink_commit();
        });
        globals.set("__ink_commit", commit)?;
        
        // __ink_is_dirty
        let is_dirty = rquickjs::Function::new(ctx.clone(), || {
            ffi::__ink_is_dirty()
        });
        globals.set("__ink_is_dirty", is_dirty)?;
        
        // __ink_clear_dirty
        let clear_dirty = rquickjs::Function::new(ctx.clone(), || {
            ffi::__ink_clear_dirty();
        });
        globals.set("__ink_clear_dirty", clear_dirty)?;
        
        // __ink_measure_text
        let measure_text = rquickjs::Function::new(ctx.clone(), |text: String, width: f64| {
            let (w, h) = ffi::__ink_measure_text(&text, width as u32);
            format!("{},{}", w, h)
        });
        globals.set("__ink_measure_text", measure_text)?;
        
        // __ink_measure_element
        let measure_element = rquickjs::Function::new(ctx.clone(), |id: String| {
            let id = id.parse::<f64>().unwrap_or(0.0) as u32;
            match ffi::__ink_measure_element(id) {
                Some((w, h)) => format!("{},{}", w, h),
                None => "null".to_string(),
            }
        });
        globals.set("__ink_measure_element", measure_element)?;
        
        // __ink_exit
        let exit = rquickjs::Function::new(ctx.clone(), |code: f64| {
            ffi::__ink_exit(code as i32);
        });
        globals.set("__ink_exit", exit)?;
        
        // __ink_should_exit
        let should_exit = rquickjs::Function::new(ctx.clone(), || {
            ffi::__ink_should_exit()
        });
        globals.set("__ink_should_exit", should_exit)?;
        
        // __ink_get_exit_code
        let get_exit_code = rquickjs::Function::new(ctx.clone(), || {
            ffi::__ink_get_exit_code() as f64
        });
        globals.set("__ink_get_exit_code", get_exit_code)?;
        
        // __ink_reset_exit
        let reset_exit = rquickjs::Function::new(ctx.clone(), || {
            ffi::__ink_reset_exit();
        });
        globals.set("__ink_reset_exit", reset_exit)?;
        
        // __ink_set_terminal_size
        let set_terminal_size = rquickjs::Function::new(ctx.clone(), |width: f64, height: f64| {
            ffi::__ink_set_terminal_size(width as u32, height as u32);
        });
        globals.set("__ink_set_terminal_size", set_terminal_size)?;
        
        // __ink_get_terminal_size
        let get_terminal_size = rquickjs::Function::new(ctx.clone(), || {
            let (w, h) = ffi::__ink_get_terminal_size();
            format!("{},{}", w, h)
        });
        globals.set("__ink_get_terminal_size", get_terminal_size)?;
        
        // __ink_get_node_tag
        let get_node_tag = rquickjs::Function::new(ctx.clone(), |id: String| {
            let id = id.parse::<f64>().unwrap_or(0.0) as u32;
            ffi::__ink_get_node_tag(id).unwrap_or_else(|| "null".to_string())
        });
        globals.set("__ink_get_node_tag", get_node_tag)?;
        
        // __ink_get_node_text
        let get_node_text = rquickjs::Function::new(ctx.clone(), |id: String| {
            let id = id.parse::<f64>().unwrap_or(0.0) as u32;
            ffi::__ink_get_node_text(id).unwrap_or_else(|| "null".to_string())
        });
        globals.set("__ink_get_node_text", get_node_text)?;
        
        // __ink_get_node_children
        let get_node_children = rquickjs::Function::new(ctx.clone(), |id: String| {
            let id = id.parse::<f64>().unwrap_or(0.0) as u32;
            match ffi::__ink_get_node_children(id) {
                Some(children) => {
                    let s: Vec<String> = children.iter().map(|&c| c.to_string()).collect();
                    format!("[{}]", s.join(","))
                }
                None => "null".to_string(),
            }
        });
        globals.set("__ink_get_node_children", get_node_children)?;
        
        // __ink_get_node_prop
        let get_node_prop = rquickjs::Function::new(ctx.clone(), |id: String, prop: String| {
            let id = id.parse::<f64>().unwrap_or(0.0) as u32;
            ffi::__ink_get_node_prop(id, &prop).unwrap_or_else(|| "null".to_string())
        });
        globals.set("__ink_get_node_prop", get_node_prop)?;
        
        // __ink_get_root_id
        let get_root_id = rquickjs::Function::new(ctx.clone(), || {
            match ffi::__ink_get_root_id() {
                Some(id) => id.to_string(),
                None => "null".to_string(),
            }
        });
        globals.set("__ink_get_root_id", get_root_id)?;
        
        // __ink_calculate_layout
        let calculate_layout = rquickjs::Function::new(ctx.clone(), || {
            ffi::__ink_calculate_layout().is_ok()
        });
        globals.set("__ink_calculate_layout", calculate_layout)?;
        
        // Console polyfill
        let console = rquickjs::Object::new(ctx.clone())?;
        console.set("log", rquickjs::Function::new(ctx.clone(), move |args: Vec<rquickjs::Value>| {
            let output: Vec<String> = args.iter()
                .map(|v| format!("{:?}", v))
                .collect();
            println!("{}", output.join(" "));
        }))?;
        console.set("error", rquickjs::Function::new(ctx.clone(), move |args: Vec<rquickjs::Value>| {
            let output: Vec<String> = args.iter()
                .map(|v| format!("{:?}", v))
                .collect();
            eprintln!("{}", output.join(" "));
        }))?;
        console.set("warn", rquickjs::Function::new(ctx.clone(), move |args: Vec<rquickjs::Value>| {
            let output: Vec<String> = args.iter()
                .map(|v| format!("{:?}", v))
                .collect();
            eprintln!("[WARN] {}", output.join(" "));
        }))?;
        globals.set("console", console)?;
        
        // Process polyfill (minimal)
        let process = rquickjs::Object::new(ctx.clone())?;
        let stdout_obj = rquickjs::Object::new(ctx.clone())?;
        stdout_obj.set("write", rquickjs::Function::new(ctx.clone(), move |s: String| {
            print!("{}", s);
            let _ = std::io::stdout().flush();
        }))?;
        process.set("stdout", stdout_obj)?;
        let stderr_obj = rquickjs::Object::new(ctx.clone())?;
        stderr_obj.set("write", rquickjs::Function::new(ctx.clone(), move |s: String| {
            eprint!("{}", s);
        }))?;
        process.set("stderr", stderr_obj)?;
        globals.set("process", process)?;
        
        Ok::<_, rquickjs::Error>(())
    })?;
    
    // Create terminal
    tracing::debug!("Initializing terminal");
    crossterm::terminal::enable_raw_mode()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    terminal.clear()?;
    
    // Create event stream
    let mut event_stream = EventStream::new();
    
    // Initialize root node
    let root_id = ffi::__ink_create_root();
    tracing::info!("Created root node: {}", root_id);
    
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
    
    // If not interactive, just run the initial render and exit cleanly
    if !interactive {
        tracing::info!("Non-interactive mode: rendering and exiting");
        
        // Do initial render
        if let Err(e) = render_tree(&mut terminal, Some(root_id)) {
            tracing::error!("Render error: {:?}", e);
        }
        
        // Cleanup
        crossterm::terminal::disable_raw_mode()?;
        terminal.show_cursor()?;
        
        tracing::info!("TuiBridge shutting down");
        return Ok(());
    }
    
    // Run the event loop
    tracing::info!("Starting event loop");
    
    let mut dirty = true;
    
    loop {
        // Check for exit
        if ffi::__ink_should_exit() {
            tracing::info!("Exit requested, code: {}", ffi::__ink_get_exit_code());
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
                        
                        // Dispatch to JS - evaluate callback code
                        let callback_js = ffi::__ink_dispatch_key(&key_str, ctrl, shift, alt);
                        if !callback_js.is_empty() && callback_js != "[]" {
                            ctx.with(|ctx| {
                                let code = format!("try {{ {} }} catch(e) {{ console.error(e) }}", callback_js);
                                if let Err(e) = ctx.eval::<(), _>(&*code) {
                                    tracing::warn!("Callback error: {:?}", e);
                                }
                            });
                        }
                        
                        // Check if JS rendered anything
                        dirty = dirty || ffi::__ink_is_dirty();
                    }
                    Some(Ok(Event::Mouse(_))) => {
                        // TODO: Handle mouse events
                    }
                    Some(Ok(Event::Resize(cols, rows))) => {
                        tracing::debug!("Terminal resize: {}x{}", cols, rows);
                        ffi::__ink_set_terminal_size(cols as u32, rows as u32);
                        dirty = true;
                    }
                    _ => {}
                }
            }
            
            // Handle timer callbacks (polled)
            _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
                // Process timers - handled by JS
                dirty = dirty || ffi::__ink_is_dirty();
            }
        }
        
        // Render if dirty
        if dirty {
            if let Err(e) = render_tree(&mut terminal, Some(root_id)) {
                tracing::error!("Render error: {:?}", e);
            }
            ffi::__ink_clear_dirty();
            dirty = false;
        }
    }
    
    // Cleanup
    crossterm::terminal::disable_raw_mode()?;
    terminal.show_cursor()?;
    
    tracing::info!("TuiBridge shutting down");
    Ok(())
}
