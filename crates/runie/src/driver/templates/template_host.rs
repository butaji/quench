//! # Host Templates
//!
//! Templates for the host crate.

/// Host main.rs template.
pub const HOST_MAIN: &str = r#"//! # Host Binary
//!
//! Thin host binary that loads and manages the app dylib.

use std::path::PathBuf;
use libloading::Library;
use protocol::{App, AppState};
use ratatui::{Terminal, backend::CrosstermBackend};

/// Application loader that manages dylib loading.
pub struct AppLoader {
    /// Loaded library
    lib: Option<Library>,
    /// App creator function
    creator: Option<unsafe extern "C" fn() -> *mut dyn App>,
}

impl AppLoader {
    /// Load a dylib from path.
    /// # Safety
    /// The path must point to a valid rune-generated dylib.
    #[allow(unsafe_code)]
    pub unsafe fn load(path: &PathBuf) -> Result<Self, libloading::Error> {
        let lib = Library::new(path)?;
        let creator: libloading::Symbol<
            unsafe extern "C" fn() -> *mut dyn App
        > = lib.get(b"create_app")?;
        Ok(Self {
            lib: Some(lib),
            creator: Some(*creator),
        })
    }

    /// Create a new app instance.
    ///
    /// # Panics
    /// Panics if `create_app` returns a null pointer.
    #[must_use]
    pub fn create_app(&self) -> Box<dyn App> {
        unsafe {
            let creator = self.creator.expect("creator not set");
            let ptr = creator();
            if ptr.is_null() {
                panic!("create_app() returned null pointer - dylib may be malformed");
            }
            Box::from_raw(ptr)
        }
    }

    /// Check if loader is valid.
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        self.lib.is_some()
    }
}

/// Run the host event loop.
pub fn run_host() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = setup_terminal()?;
    let mut state = AppState::default();
    let mut app_loader: Option<AppLoader> = None;
    let mut current_app: Option<Box<dyn App>> = None;
    let hot_dir = PathBuf::from("target/hot");
    let current_link = hot_dir.join(".current");

    loop {
        // Check for dylib reload
        let (new_loader, new_app) = check_and_reload(&current_link, app_loader.take(), current_app.take());
        app_loader = new_loader;
        current_app = new_app;

        // Run one frame with current app
        if let Some(ref mut app) = current_app {
            run_app_frame(app, &mut state, &mut terminal);
        }

        // Handle input events
        if let Some(should_exit) = handle_events(current_app.as_ref(), &mut state) {
            if should_exit {
                break;
            }
        }

        if state.should_exit {
            break;
        }
    }

    cleanup_terminal();
    Ok(())
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<std::io::Stdout>>, Box<dyn std::error::Error>> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend))
}

#[allow(unsafe_code)]
fn check_and_reload(
    current_link: &PathBuf,
    app_loader: Option<AppLoader>,
    _current_app: Option<Box<dyn App>>,
) -> (Option<AppLoader>, Option<Box<dyn App>>) {
    if !current_link.exists() {
        return (app_loader, None);
    }

    if let Ok(target) = std::fs::read_link(current_link) {
        let needs_reload = app_loader
            .as_ref()
            .and_then(|l| l.lib.as_ref())
            .map(|lib| lib.path() != Some(target.clone()))
            .unwrap_or(true);

        if needs_reload {
            if let Ok(loader) = unsafe { AppLoader::load(&target) } {
                let app = Some(loader.create_app());
                return (Some(loader), app);
            }
        } else if let Some(loader) = &app_loader {
            // Same dylib, reuse app instance
            return (app_loader, Some(loader.create_app()));
        }
    }
    (app_loader, None)
}

fn run_app_frame(
    app: &mut dyn App,
    state: &mut AppState,
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) {
    app.update(state);
    let _ = terminal.draw(|f| {
        app.render(f, state);
    });
}

fn handle_events(
    app: Option<&dyn App>,
    state: &mut AppState,
) -> Option<bool> {
    // Use non-blocking poll for hot reload compatibility
    if crossterm::event::poll(std::time::Duration::from_millis(50)).unwrap_or(false) {
        if let Ok(crossterm::event::Event::Key(key)) = crossterm::event::read() {
            if key.code == crossterm::event::KeyCode::Char('q') {
                return Some(true);
            }
            if let Some(app) = app {
                app.handle_key(key, state);
            }
        }
    }
    Some(false)
}

fn cleanup_terminal() {
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen
    );
    let _ = crossterm::terminal::disable_raw_mode();
}
"#;
