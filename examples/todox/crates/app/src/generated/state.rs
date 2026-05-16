//! Generated from state.r.ts

use protocol::{AppState, Filter, Task};

/// Create a new task.
pub fn create_task(title: &str) -> Task {
    Task {
        id: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i32,
        title: title.to_string(),
        done: false,
    }
}

/// Toggle task completion.
pub fn toggle_task(task: &mut Task) {
    task.done = !task.done;
}

/// Validate task title.
pub fn validate_title(title: &str) -> Result<&str, String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        Err("Title cannot be empty".to_string())
    } else if trimmed.len() > 100 {
        Err("Title too long (max 100 chars)".to_string())
    } else {
        Ok(trimmed)
    }
}
