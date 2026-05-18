//! # App Library
//!
//! Hot-reloadable application logic.

mod native;

use protocol::{App, AppState};

/// Create a new app instance.
#[no_mangle]
pub extern "C" fn create_app() -> *mut dyn App {
    Box::into_raw(Box::new(AppImpl::default()))
}

#[derive(Default)]
struct AppImpl;

impl App for AppImpl {
    fn update(&mut self, state: &mut AppState) {
        generated::main::update(state);
    }

    fn render(&self, f: &mut ratatui::Frame<'_>, state: &AppState) {
        generated::main::render(f, state);
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent, state: &mut AppState) {
        generated::main::handle_key(key, state);
    }
}
