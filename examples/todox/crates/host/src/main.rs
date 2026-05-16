//! # Host Binary
//!
//! Thin binary that owns AppState and loads the app dylib.
//! ~80 lines of code, rarely needs editing.

use anyhow::{Context, Result};
use crossterm::event::{Event, KeyCode, KeyEventKind};
use crossterm::{event, terminal};
use libloading::{Library, Symbol};
use protocol::{App, AppState};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{error, info};

/// Load the app dylib.
fn load_dylib(path: &Path) -> Result<Box<dyn App>> {
    unsafe {
        let lib = Library::new(path).context("Failed to load dylib")?;
        let create_app: Symbol<unsafe extern "C" fn() -> *mut dyn App> =
            lib.get(b"create_app").context("Failed to find create_app")?;
        let app = create_app();
        Ok(Box::from_raw(app))
    }
}

/// Get the current dylib path.
fn current_dylib() -> PathBuf {
    PathBuf::from("target/hot/.current")
}

/// Check for protocol changes (restart needed).
fn check_protocol_change() -> bool {
    PathBuf::from("target/hot/.restart-needed").exists()
}

fn main() -> Result<()> {
    // Initialize terminal
    terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, terminal::EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Load initial dylib
    let dylib_path = current_dylib();
    info!("Loading dylib: {:?}", dylib_path);

    let mut app = if dylib_path.exists() {
        load_dylib(&dylib_path)?
    } else {
        error!("No dylib found at {:?}", dylib_path);
        return Ok(());
    };

    // Main event loop
    let mut state = AppState::default();
    let tick_rate = Duration::from_millis(100);

    loop {
        // Render
        terminal.draw(|f| {
            app.render(f, &state);
        })?;

        // Handle events
        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            state.should_exit = true;
                        }
                        _ => {}
                    }
                    app.handle_key(key, &mut state);
                }
            }
        }

        // Update
        app.update(&mut state);

        // Check for exit
        if state.should_exit {
            break;
        }

        // Check for dylib reload
        if let Ok(new_path) = std::fs::read_link(&dylib_path) {
            // Reload if changed
            if new_path != dylib_path {
                info!("Reloading dylib: {:?}", new_path);
                app = load_dylib(&new_path)?;
            }
        }
    }

    // Cleanup
    terminal::disable_raw_mode()?;
    let _ = crossterm::execute!(terminal.backend_mut(), terminal::LeaveAlternateScreen);

    Ok(())
}
