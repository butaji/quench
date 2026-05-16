//! Generated from main.r.ts

use protocol::{AppState, Filter, Task};

/// Update application state.
pub fn update(state: &mut AppState) {
    // Filter tasks based on current filter
    let filtered = filter_tasks(&state.tasks, state.filter);

    // Clamp selected index
    if state.selected >= filtered.len() && !filtered.is_empty() {
        state.selected = filtered.len() - 1;
    }
}

/// Filter tasks by completion status.
pub fn filter_tasks(tasks: &[Task], filter: Filter) -> Vec<Task> {
    match filter {
        Filter::Active => tasks.iter().filter(|t| !t.done).cloned().collect(),
        Filter::Completed => tasks.iter().filter(|t| t.done).cloned().collect(),
        Filter::All => tasks.to_vec(),
    }
}

/// Get statistics about tasks.
pub fn get_stats(tasks: &[Task]) -> TaskStats {
    let done = tasks.iter().filter(|t| t.done).count();
    TaskStats {
        total: tasks.len(),
        done,
        active: tasks.len() - done,
    }
}

#[derive(Debug, Clone)]
pub struct TaskStats {
    pub total: usize,
    pub done: usize,
    pub active: usize,
}

/// Find task by ID.
pub fn find_task<'a>(tasks: &'a [Task], id: i32) -> Option<&'a Task> {
    tasks.iter().find(|t| t.id == id)
}

/// Sort tasks by various criteria.
pub fn sort_tasks(tasks: &mut [Task], by: SortBy) {
    match by {
        SortBy::Id => tasks.sort_by_key(|t| t.id),
        SortBy::Title => tasks.sort_by(|a, b| a.title.cmp(&b.title)),
        SortBy::Done => tasks.sort_by(|a, b| a.done.cmp(&b.done)),
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SortBy {
    Id,
    Title,
    Done,
}
