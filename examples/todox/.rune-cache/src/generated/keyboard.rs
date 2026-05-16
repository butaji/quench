// Module: keyboard.r

use protocol::{AppState, Filter, Task};
use ratatui::widgets::{Widget, Paragraph, ListItem, Span};
use ratatui::style::Style;
use crossterm::event::KeyCode;
use serde_json;

use crate::native;

pub fn handle_navigation(key: KeyCode, state: AppState) -> () {
    // switch
}

pub fn handle_task_action(key: KeyCode, state: AppState) -> () {
    // switch
}

pub fn toggle_selected_task(state: AppState) -> () {
    let task: () = state.tasks.get(state.selected);
    if task {
        task.done = !task.done;
    }
}

pub fn delete_selected_task(state: AppState) -> () {
    state.tasks.splice(state.selected..state.selected + 1i32, vec![]);
    if state.selected >= state.tasks.length && state.tasks.length > 0i32 {
        state.selected = state.tasks.length - 1i32;
    }
}

pub fn add_new_task(state: AppState) -> () {
    let task: () = { id: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64, title: "New Task", done: false };
    state.tasks.push(task);
    state.selected = state.tasks.length - 1i32;
}

