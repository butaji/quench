//! App library - hot-reloadable app logic.
//!
//! Hand-written wiring layer (~20 lines).
//! Logic lives in .r.ts and .r.tsx files under src/.

#![allow(improper_ctypes_definitions)]

mod native;

pub mod generated;

use protocol::{App, AppState};

/// Create new app instance - called by host binary.
#[no_mangle]
pub extern "C" fn create_app() -> *mut dyn App {
    Box::into_raw(Box::new(AppImpl))
}

struct AppImpl;

impl Default for AppImpl {
    fn default() -> Self {
        Self
    }
}

impl App for AppImpl {
    fn update(&mut self, state: &mut AppState) {
        crate::generated::main::update(state);
    }

    fn render(&self, f: &mut ratatui::Frame<'_>, state: &AppState) {
        crate::generated::views::root::render(f, state);
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent, state: &mut AppState) {
        crate::native::handlers::handle_key_native(key, state);
    }
}
