//! Generated from handlers/keyboard.r.ts

use protocol::{AppState, Task};

/// Toggle the currently selected task.
pub fn toggle_selected_task(state: &mut AppState) {
    if let Some(task) = state.tasks.get_mut(state.selected) {
        task.done = !task.done;
    }
}

/// Delete the currently selected task.
pub fn delete_selected_task(state: &mut AppState) {
    state.tasks.remove(state.selected);
    if state.selected >= state.tasks.len() && !state.tasks.is_empty() {
        state.selected = state.tasks.len() - 1;
    }
}

/// Add a new task.
pub fn add_new_task(state: &mut AppState) {
    let task = Task {
        id: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i32,
        title: "New Task".to_string(),
        done: false,
    };
    state.tasks.push(task);
    state.selected = state.tasks.len() - 1;
}
