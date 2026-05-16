//! # App Library
//!
//! Hot-reloadable application logic.
//! This file is auto-generated - edit main.r.ts instead.

mod native;
mod generated;

// Re-export generated modules
pub use generated::*;

/// Create the app instance.
#[no_mangle]
pub extern "C" fn create_app() -> *mut dyn protocol::App {
    Box::into_raw(Box::new(AppImpl::default()))
}

/// App implementation.
#[derive(Default)]
struct AppImpl;

impl protocol::App for AppImpl {
    fn update(&mut self, state: &mut protocol::AppState) {
        generated::main::update(state);
    }

    fn render(&self, f: &mut ratatui::Frame<'_>, state: &protocol::AppState) {
        let widget = generated::views::root::render(state);
        f.render_widget(widget, f.size());
    }

    fn handle_key(
        &mut self,
        key: crossterm::event::KeyEvent,
        state: &mut protocol::AppState,
    ) {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if state.selected > 0 {
                    state.selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if state.selected < state.tasks.len().saturating_sub(1) {
                    state.selected += 1;
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(task) = state.tasks.get_mut(state.selected) {
                    task.done = !task.done;
                }
            }
            KeyCode::Char('a') => {
                state.tasks.push(protocol::Task {
                    id: rand_id(),
                    title: "New Task".to_string(),
                    done: false,
                });
            }
            KeyCode::Char('d') => {
                state.tasks.remove(state.selected);
                state.selected = state.selected.saturating_sub(1);
            }
            KeyCode::Char('f') => {
                state.filter = match state.filter {
                    protocol::Filter::All => protocol::Filter::Active,
                    protocol::Filter::Active => protocol::Filter::Completed,
                    protocol::Filter::Completed => protocol::Filter::All,
                };
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                state.should_exit = true;
            }
            _ => {}
        }
    }
}

/// Generate a random ID.
fn rand_id() -> i32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i32
}
