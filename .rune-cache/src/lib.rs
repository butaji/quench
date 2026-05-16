//! Generated Rune modules

mod native;

pub mod generated;

use protocol::{App, AppState};

// Re-export types
pub use protocol::{Filter, Task};


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

    fn render(&self, frame: &mut ratatui::Frame, state: &AppState) {
        let widget = generated::root::render(state);
        frame.render_widget(widget, frame.size());
    }

    fn handle_key(&mut self, _key: crossterm::event::KeyEvent, _state: &mut AppState) {}
}
