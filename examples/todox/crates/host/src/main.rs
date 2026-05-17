//! # Host Binary
//!
//! Thin host binary that loads and manages the app dylib.
//! State owner - survives dylib reloads.

#![allow(dead_code)]

use std::path::PathBuf;
use libloading::Library;
use protocol::{App, AppState};
use ratatui::{Terminal, backend::CrosstermBackend};
use crossterm::event::{Event, KeyCode, KeyEvent};

/// Loads and manages the app dylib.
pub struct AppLoader {
    /// Library handle
    #[allow(dead_code)]
    lib: Option<Library>,
    /// Path to loaded library
    path: PathBuf,
    /// Creator function
    creator: Option<unsafe fn() -> *mut dyn App>,
}

impl AppLoader {
    /// Load a new dylib.
    pub unsafe fn load(path: &PathBuf) -> Result<Self, libloading::Error> {
        let lib = Library::new(path)?;
        let creator: libloading::Symbol<unsafe fn() -> *mut dyn App> =
            lib.get(b"create_app")?;
        let creator_fn = *creator;
        Ok(Self {
            lib: Some(lib),
            path: path.clone(),
            creator: Some(creator_fn),
        })
    }

    /// Create a new app instance.
    #[allow(unused)]
    pub fn create_app(&self) -> Option<Box<dyn App>> {
        unsafe {
            let creator = self.creator?;
            Some(Box::from_raw(creator()))
        }
    }

    /// Check if this loader has a specific path.
    pub fn has_path(&self, path: &PathBuf) -> bool {
        &self.path == path
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup terminal
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // State lives in host - survives dylib reloads
    let mut state = AppState::default();
    let hot_dir = PathBuf::from("target/hot");
    let current_link = hot_dir.join(".current");

    // Main event loop
    loop {
        // Check for new dylib
        if current_link.exists() {
            if let Ok(target) = std::fs::read_link(&current_link) {
                terminal.draw(|f| {
                    if let Some(app) = load_app(&target, &mut state) {
                        app.render(f, &state);
                    }
                })?;
            }
        }

        // Handle input
        if let Ok(Event::Key(KeyEvent { code, .. })) = crossterm::event::read() {
            match code {
                KeyCode::Char('q') => break,
                _ => handle_key(&current_link, code, &mut state),
            }
        }

        if state.should_exit {
            break;
        }
    }

    // Cleanup
    let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);
    crossterm::terminal::disable_raw_mode()?;
    Ok(())
}

/// Load app dylib or return cached state.
#[allow(unused_variables)]
fn load_app(target: &PathBuf, state: &mut AppState) -> Option<Box<dyn App>> {
    // For simplicity, return None - in real implementation
    // this would load/reload the dylib
    None
}

/// Handle key events.
#[allow(unused_variables)]
fn handle_key(current_link: &PathBuf, code: KeyCode, state: &mut AppState) {
    // Key handling would be implemented here
}
