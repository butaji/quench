//! task_list.r.tsx - Task list component
//!
//! Displays a filtered list of tasks.

import { Task, Filter } from "../state.r.ts";
import { task_matches_filter } from "../main.r.ts";

/// Task list view props.
export type TaskListProps = {
    tasks: Task[];
    selected: number;
    filter: Filter;
};

/// Render a list of filtered tasks.
export function task_list(props: TaskListProps): TaskItem[] {
    const items: TaskItem[] = [];
    
    for (let i = 0; i < props.tasks.length; i++) {
        const task = props.tasks[i];
        if (task_matches_filter(task, props.filter)) {
            items.push({
                task,
                index: i,
                is_selected: i === props.selected,
            });
        }
    }
    
    return items;
}

/// A single task item for rendering.
export type TaskItem = {
    task: Task;
    index: number;
    is_selected: boolean;
};

/// Get the display text for a task item.
export function task_display_text(item: TaskItem): string {
    const checkbox = item.task.done ? "[x]" : "[ ]";
    const prefix = item.is_selected ? "> " : "  ";
    return `${prefix}${checkbox} ${item.task.title}`;
}
