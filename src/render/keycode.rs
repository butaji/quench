//! Key code conversion utilities

use crossterm::event::KeyCode;

/// Convert crossterm KeyCode to Ink-compatible key name
pub fn keycode_to_ink_name(key: &crossterm::event::KeyEvent) -> String {
    match key.code {
        KeyCode::Char(' ') => " ".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "return".to_string(),
        KeyCode::Esc => "escape".to_string(),
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Delete => "delete".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::Up => "upArrow".to_string(),
        KeyCode::Down => "downArrow".to_string(),
        KeyCode::Left => "leftArrow".to_string(),
        KeyCode::Right => "rightArrow".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::PageUp => "pageUp".to_string(),
        KeyCode::PageDown => "pageDown".to_string(),
        KeyCode::Insert => "insert".to_string(),
        KeyCode::BackTab => "tab".to_string(),
        KeyCode::F(n) => format!("f{}", n),
        _ => format!("{:?}", key.code).to_lowercase(),
    }
}
