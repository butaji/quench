// Module: state.r

use protocol::{AppState, Filter, Task};
use ratatui::widgets::{Widget, Paragraph, ListItem, Span};
use ratatui::style::Style;
use crossterm::event::KeyCode;
use serde_json;

use crate::native;

pub fn create_task(title: String) -> Task {
    return { id: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64, title: title, done: false };
}

pub fn toggle_task(task: Task) -> Task {
    return { ..task, done: !task.done };
}

pub fn validate_title(title: String) -> Result {
    let trimmed: String = title.trim();
    if trimmed.len() == 0i32 {
        return { ok: false, error: "Title cannot be empty" };
    }
    if trimmed.len() > 100i32 {
        return { ok: false, error: "Title too long (max 100 chars)" };
    }
    return { ok: true, value: trimmed };
}

