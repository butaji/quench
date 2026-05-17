//! Host binary - loads and manages the app dylib.
//!
//! Thin state owner (~80 lines). Rarely edited.

#![allow(improper_ctypes_definitions)]

use libloading::Library;
use protocol::{App, AppState};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::path::PathBuf;

/// Wrapper for loaded app dylib.
struct AppLoader {
    lib: Option<Library>,
    creator: unsafe extern "C" fn() -> *mut dyn App,
}

impl AppLoader {
    /// Load app from path.
    /// # Safety
    /// Path must point to valid rune-generated dylib.
    #[must_use]
    unsafe fn load(path: &PathBuf) -> Result<Self, libloading::Error> {
        let lib = Library::new(path)?;
        let creator: libloading::Symbol<unsafe extern "C" fn() -> *mut dyn App> =
            lib.get(b"create_app")?;
        let creator_fn = *creator;
        Ok(Self {
            lib: Some(lib),
            creator: creator_fn,
        })
    }

    /// Create new app instance.
    #[must_use]
    fn create_app(&self) -> Box<dyn App> {
        unsafe {
            let ptr = (self.creator)();
            Box::from_raw(ptr)
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup terminal
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // State and loader
    let mut state = AppState::default();
    let mut app_loader: Option<AppLoader> = None;

    // Hot directory
    let hot_dir = PathBuf::from("target/hot");
    let current_link = hot_dir.join(".current");

    // Main loop
    loop {
        // Check for new dylib
        if current_link.exists() {
            if let Ok(target) = std::fs::read_link(&current_link) {
                // Simple reload on every change for demo
                app_loader = None;
                unsafe {
                    if let Ok(loader) = AppLoader::load(&target) {
                        app_loader = Some(loader);
                    }
                }
            }
        }

        // Update and render
        if let Some(ref loader) = app_loader {
            let mut app = loader.create_app();
            app.update(&mut state);
            terminal.draw(|f| app.render(f, &state))?;
        }

        // Handle events
        if let Ok(event) = crossterm::event::read() {
            if let crossterm::event::Event::Key(key) = event {
                if key.code == crossterm::event::KeyCode::Char('q') {
                    break;
                }
                if let Some(ref loader) = app_loader {
                    let mut app = loader.create_app();
                    app.handle_key(key, &mut state);
                }
            }
        }

        if state.should_exit {
            break;
        }
    }

    // Cleanup
    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    crossterm::terminal::disable_raw_mode()?;

    Ok(())
}
