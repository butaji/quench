//! # Host Binary
//!
//! Thin host binary that loads and manages the app dylib.
//! State owner - survives dylib reloads.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Main entry point.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup terminal
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // State lives in host - survives dylib reloads
    let mut state = AppState::default();
    let hot_dir = find_workspace_hot_dir();

    if !hot_dir.exists() {
        eprintln!("Error: target/hot directory not found. Run `cargo rune dev` first.");
        cleanup_terminal();
        return Ok(());
    }

    let mut current_lib: Option<libloading::Library> = None;
    let mut current_path: Option<PathBuf> = None;
    let mut current_app: Option<Box<dyn App>> = None;

    // Main event loop
    loop {
        // Check for dylib changes
        let target = hot_dir.join(".current");
        if target.exists() {
            if let Ok(path) = fs::read_link(&target) {
                let needs_reload = match &current_path {
                    Some(p) => p != &path,
                    None => true,
                };

                if needs_reload {
                    // Drop old library (releases handle)
                    drop(current_app.take());
                    drop(current_lib.take());
                    current_path = None;

                    // Load new dylib
                    match unsafe { load_app(&path) } {
                        Ok(app) => {
                            current_app = Some(app);
                            current_path = Some(path);
                        }
                        Err(e) => {
                            eprintln!("Failed to load dylib: {e}");
                        }
                    }
                }
            }
        }

        // Render
        if let Some(ref app) = current_app {
            terminal.draw(|f| {
                app.render(f, &state);
            })?;
        } else {
            terminal.draw(|f| {
                let area = f.size();
                let text = ratatui::widgets::Paragraph::new("Waiting for build...");
                f.render_widget(text, area);
            })?;
        }

        // Handle input with timeout
        let event = crossterm::event::poll(Duration::from_millis(100));

        if event.is_ok() {
            if let Ok(event) = crossterm::event::read() {
                match event {
                    crossterm::event::Event::Key(key) => {
                        if key.code == crossterm::event::KeyCode::Char('q') {
                            break;
                        }
                        if let Some(ref mut app) = current_app {
                            app.handle_key(key, &mut state);
                        }
                        if state.should_exit {
                            break;
                        }
                    }
                    crossterm::event::Event::Resize(_, _) => {
                        // Terminal resize handled by crossterm
                    }
                    _ => {}
                }
            }
        }

        // Check for protocol restart
        if hot_dir.join("restart_needed").exists() {
            eprintln!("Protocol changed. Full restart required.");
            eprintln!("Run `cargo rune dev` again.");
            break;
        }
    }

    cleanup_terminal();
    Ok(())
}

/// Load app from dylib path.
unsafe fn load_app(path: &Path) -> Result<Box<dyn App>, libloading::Error> {
    let lib = libloading::Library::new(path)?;
    let creator: libloading::Symbol<unsafe fn() -> *mut dyn App> =
        lib.get(b"create_app")?;
    Ok(Box::from_raw(creator()))
}

/// Find the hot directory relative to current directory.
fn find_workspace_hot_dir() -> PathBuf {
    let current = std::env::current_dir().unwrap_or_default();
    current.join("target/hot")
}

/// Cleanup terminal on exit.
fn cleanup_terminal() {
    let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);
    let _ = crossterm::terminal::disable_raw_mode();
}

// Re-export for convenience
use protocol::{App, AppState};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::fs;
