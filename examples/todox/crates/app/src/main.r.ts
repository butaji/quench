//! main.r.ts - Main entry point for the app logic
//!
//! This is the Rune entry point that gets transpiled to Rust.

import { AppState, Task, Filter } from "./state.r.ts";
import { handle_key as kb_handle } from "./handlers/keyboard.r.ts";

/// Update application state.
/// Called every frame by the host.
export function update(state: AppState): void {
    if (state.selected >= state.tasks.length) {
        state.selected = Math.max(0, state.tasks.length - 1);
    }
}

/// Check if task matches the current filter.
export function task_matches_filter(task: Task, filter: Filter): boolean {
    switch (filter) {
        case Filter.Active:
            return !task.done;
        case Filter.Completed:
            return task.done;
        default:
            return true;
    }
}

/// Get filtered task count.
export function get_filtered_count(state: AppState): number {
    let count = 0;
    for (const task of state.tasks) {
        if (task_matches_filter(task, state.filter)) {
            count++;
        }
    }
    return count;
}
