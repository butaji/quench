//! # TodoX Host Binary
//!
//! Thin host binary (~80 lines) that loads the app dylib and runs the TUI event loop.
//! This is the state owner - AppState lives here and survives dylib hot reloads.

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::sync::{Arc, Mutex};

mod app_loader;

use app_loader::AppLoader;

/// Application state that survives hot reloads.
/// Serialized before reload, deserialized after.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AppState {
    pub tasks: Vec<Task>,
    pub selected: usize,
    pub filter: String,
    pub should_exit: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Task {
    pub id: u32,
    pub title: String,
    pub done: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            tasks: Vec::new(),
            selected: 0,
            filter: "all".to_string(),
            should_exit: false,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app_loader = AppLoader::new()?;
    let state: Arc<Mutex<AppState>> = Arc::new(Mutex::new(AppState::default()));

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Main event loop
    let res = run_app(&mut app_loader, &state, &mut terminal);

    // Cleanup
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    if let Err(e) = res {
        eprintln!("Error: {e:?}");
    }
    Ok(())
}

fn run_app(
    app_loader: &mut AppLoader,
    state: &Arc<Mutex<AppState>>,
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        terminal.draw(|f| {
            let state = state.lock().unwrap();
            if let Some(render_fn) = app_loader.get_render_fn() {
                render_fn(f, &state);
            }
        })?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    let should_reload = app_loader.handle_key(&key);
                    if should_reload {
                        app_loader.reload()?;
                    }
                    let mut state = state.lock().unwrap();
                    if let Some(key_fn) = app_loader.get_key_fn() {
                        key_fn(&key, &mut state);
                    }
                    if state.should_exit {
                        return Ok(());
                    }
                }
            }
        }
    }
}
