//! Rune application wiring layer

mod native;

use protocol::{App, AppState};

#[no_mangle]
pub extern "C" fn create_app() -> *mut dyn App {
    Box::into_raw(Box::new(AppImpl::default()))
}

#[derive(Default)]
struct AppImpl;

impl App for AppImpl {
    fn update(&mut self, state: &mut AppState) {
        // Call the generated update function
        generated::main::update(state);
    }

    fn render(&self, frame: &mut ratatui::Frame, state: &AppState) {
        use ratatui::{widgets::Paragraph, layout::Rect};
        let count = state.tasks.len();
        let text = format!("TODOX - {} tasks", count);
        let paragraph = Paragraph::new(text);
        frame.render_widget(paragraph, Rect::new(0, 0, 30, 1));
    }

    fn handle_key(&mut self, _key: crossterm::event::KeyEvent, _state: &mut AppState) {}
}
