//! # View Templates
//!
//! Templates for TSX views.

/// Root view template.
pub const ROOT_RSX: &str = r#"//! root.r.tsx - Main view component

import { AppState } from "../state.r.ts";

/// Render the root view.
export function render(f: Frame, state: AppState): void {
    let content = "TODOX - " + state.tasks.length + " tasks\n";
    content += "-".repeat(40) + "\n";
    for (let i = 0; i < state.tasks.length; i++) {
        const task = state.tasks[i];
        const marker = task.done ? "[x]" : "[ ]";
        const prefix = i === state.selected ? "> " : "  ";
        content += prefix + marker + " " + task.title + "\n";
    }
    content += "\nPress q to quit, j/k to navigate, x to toggle";
    // Render using ratatui
    let para = Paragraph::new(content);
    f.render_widget(para, f.size());
}
"#;

/// Task list view template.
pub const TASK_LIST_RSX: &str = r#"//! task_list.r.tsx - Task list component

import { Task, Filter } from "../state.r.ts";

/// Filter tasks by status.
export function filterTasks(tasks: Task[], filter: Filter): Task[] {
    switch (filter) {
        case Filter.Active:
            return tasks.filter(t => !t.done);
        case Filter.Completed:
            return tasks.filter(t => t.done);
        default:
            return tasks;
    }
}

/// Render task list item.
export function renderTaskItem(task: Task, selected: boolean): string {
    const marker = task.done ? "[x]" : "[ ]";
    const prefix = selected ? "> " : "  ";
    return prefix + marker + " " + task.title;
}
"#;
