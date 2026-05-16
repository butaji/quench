// Module: api.r

use protocol::{AppState, Filter, Task};
use ratatui::widgets::{Widget, Paragraph, ListItem, Span};
use ratatui::style::Style;
use crossterm::event::KeyCode;
use serde_json;

use crate::native;

#[derive(Debug, Clone)]
pub struct JsonValue {
pub _type: (),
}

pub fn is_number(val: JsonValue) -> () {
    return "unknown" == "number";
}

pub fn is_string(val: JsonValue) -> () {
    return "unknown" == "string";
}

pub fn is_boolean(val: JsonValue) -> () {
    return "unknown" == "boolean";
}

pub fn is_object(val: JsonValue) -> () {
    return "unknown" == "object" && val != None && !array.isArray(val);
}

pub fn validate_task(task: RawTask) -> Result {
    if !task {
        return { ok: false, error: "Invalid task object" };
    }
    if !is_number(task.id) {
        return { ok: false, error: "Missing or invalid id" };
    }
    if !is_string(task.title) {
        return { ok: false, error: "Missing or invalid title" };
    }
    if !is_boolean(task.done) {
        return { ok: false, error: "Missing or invalid done status" };
    }
    return { ok: true, value: { id: task.id, title: task.title.trim(), done: task.done } };
}

pub fn serialize_tasks(tasks: Vec<Task>) -> String {
    return serde_json::to_string(&tasks).unwrap_or_default();
}

pub fn parse_json(data: String) -> Result {
    let trimmed: String = data.trim();
    if !trimmed.startsWith("{") && !trimmed.startsWith("[") {
        return { ok: false, error: "Invalid JSON structure" };
    }
    return { ok: true, value: () };
}

pub fn deserialize_tasks(data: String) -> Result {
    let parse_result: () = parse_json(data);
    if !parse_result.ok {
        return { ok: false, error: parse_result.error };
    }
    let parsed: () = serde_json::from_str::<serde_json::Value>(&data).ok();
    if !array.isArray(parsed) {
        return { ok: false, error: "Expected array" };
    }
    let tasks: Vec<()> = vec![];
    // unsupported
    return { ok: true, value: tasks };
}

pub fn merge_tasks(local: Vec<Task>, remote: Vec<Task>) -> Vec<Task> {
    let merged: Vec<()> = vec![local];
    // unsupported
    return merged;
}

