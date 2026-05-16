// main.r.ts - Main entry point for the app logic

import { AppState, Task, Filter } from "./state.r.ts";

/**
 * Update application state.
 * Called by the host on each tick.
 */
export function update(state: AppState): void {
    // Filter tasks based on current filter
    const filtered = filterTasks(state.tasks, state.filter);
    
    // Clamp selected index
    if (state.selected >= filtered.length && filtered.length > 0) {
        state.selected = filtered.length - 1;
    }
}

/**
 * Filter tasks by completion status.
 */
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

/**
 * Get statistics about tasks.
 */
export function getStats(tasks: Task[]): { total: number; done: number; active: number } {
    const done = tasks.filter(t => t.done).length;
    return {
        total: tasks.length,
        done,
        active: tasks.length - done,
    };
}

/**
 * Find task by ID.
 */
export function findTask(tasks: Task[], id: number): Task | null {
    for (const task of tasks) {
        if (task.id === id) {
            return task;
        }
    }
    return null;
}

/**
 * Sort tasks by various criteria.
 */
export function sortTasks(tasks: Task[], by: "id" | "title" | "done"): Task[] {
    const sorted = [...tasks];
    switch (by) {
        case "id":
            return sorted.sort((a, b) => a.id - b.id);
        case "title":
            return sorted.sort((a, b) => a.title.localeCompare(b.title));
        case "done":
            return sorted.sort((a, b) => (a.done === b.done) ? 0 : a.done ? 1 : -1);
        default:
            return sorted;
    }
}
