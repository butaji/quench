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
    #[must_use]
    pub fn create_app(&self) -> Box<dyn App> {
        unsafe {
            let creator = self.creator.expect("creator not set");
            let ptr = creator();
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
    let hot_dir = PathBuf::from("target/hot");
    let current_link = hot_dir.join(".current");

    loop {
        app_loader = check_and_reload(&current_link, app_loader);
        
        if let Some(ref loader) = app_loader {
            run_app_frame(loader, &mut state, &mut terminal);
        }
        
        if let Some(should_exit) = handle_events(&app_loader, &mut state) {
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
    mut app_loader: Option<AppLoader>,
) -> Option<AppLoader> {
    if !current_link.exists() {
        return app_loader;
    }

    if let Ok(target) = std::fs::read_link(current_link) {
        let needs_reload = app_loader
            .as_ref()
            .and_then(|l| l.lib.as_ref())
            .map(|lib| lib.path() != Some(target.clone()))
            .unwrap_or(true);

        if needs_reload {
            app_loader = unsafe { AppLoader::load(&target).ok() };
        }
    }
    app_loader
}

fn run_app_frame(
    loader: &AppLoader,
    state: &mut AppState,
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) {
    let mut app = loader.create_app();
    app.update(state);
    let _ = terminal.draw(|f| {
        app.render(f, state);
    });
}

fn handle_events(
    app_loader: &Option<AppLoader>,
    state: &mut AppState,
) -> Option<bool> {
    if let Ok(event) = crossterm::event::read() {
        if let crossterm::event::Event::Key(key) = event {
            if key.code == crossterm::event::KeyCode::Char('q') {
                return Some(true);
            }
            if let Some(ref loader) = app_loader {
                let mut app = loader.create_app();
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
