//! Native handlers - bridge between crossterm events and app logic.

use crate::generated::main::{KeyCode, KeyEvent, update, handle_key};
use protocol::AppState;

/// Map crossterm KeyCode to app KeyCode.
pub fn map_key_code(crossterm_code: &crossterm::event::KeyCode) -> KeyCode {
    match crossterm_code {
        crossterm::event::KeyCode::Char(_) => KeyCode::Char,
        crossterm::event::KeyCode::Enter => KeyCode::Enter,
        crossterm::event::KeyCode::Esc => KeyCode::Escape,
        crossterm::event::KeyCode::Left => KeyCode::Left,
        crossterm::event::KeyCode::Right => KeyCode::Right,
        crossterm::event::KeyCode::Up => KeyCode::Up,
        crossterm::event::KeyCode::Down => KeyCode::Down,
        _ => KeyCode::Char,
    }
}

/// Handle a crossterm key event.
pub fn handle_key_native(
    key: crossterm::event::KeyEvent,
    state: &mut AppState,
) {
    let event = KeyEvent {
        code: map_key_code(&key.code),
        char: if let crossterm::event::KeyCode::Char(c) = key.code {
            Some(c.to_string())
        } else {
            None
        },
    };
    handle_key(event, state);
    update(state);
}
