//! Generated from handlers/api.r.ts

use protocol::Task;
use serde::{Deserialize, Serialize};

/// Result type for validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult<T> {
    pub ok: bool,
    pub value: Option<T>,
    pub error: Option<String>,
}

impl<T> ValidationResult<T> {
    pub fn ok(value: T) -> Self {
        Self {
            ok: true,
            value: Some(value),
            error: None,
        }
    }

    pub fn err(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            value: None,
            error: Some(error.into()),
        }
    }
}

/// Validate task data from external source.
pub fn validate_task(task: &serde_json::Value) -> ValidationResult<Task> {
    let obj = match task.as_object() {
        Some(o) => o,
        None => return ValidationResult::err("Invalid task object"),
    };

    let id = match obj.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id as i32,
        None => return ValidationResult::err("Missing or invalid id"),
    };

    let title = match obj.get("title").and_then(|v| v.as_str()) {
        Some(t) => t.trim().to_string(),
        None => return ValidationResult::err("Missing or invalid title"),
    };

    let done = match obj.get("done").and_then(|v| v.as_bool()) {
        Some(d) => d,
        None => return ValidationResult::err("Missing or invalid done status"),
    };

    ValidationResult::ok(Task { id, title, done })
}

/// Serialize tasks for storage.
pub fn serialize_tasks(tasks: &[Task]) -> String {
    serde_json::to_string(tasks).unwrap_or_default()
}

/// Deserialize tasks from storage.
pub fn deserialize_tasks(data: &str) -> ValidationResult<Vec<Task>> {
    let parsed: Vec<serde_json::Value> = match serde_json::from_str(data) {
        Ok(p) => p,
        Err(_) => return ValidationResult::err("Failed to parse JSON"),
    };

    let mut tasks = Vec::with_capacity(parsed.len());
    for item in parsed {
        match validate_task(&item) {
            ValidationResult { ok: true, value: Some(task), .. } => tasks.push(task),
            _ => return ValidationResult::err("Invalid task in array"),
        }
    }

    ValidationResult::ok(tasks)
}
