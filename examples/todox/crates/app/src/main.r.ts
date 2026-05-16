// main.r.ts - Main entry point for the app logic

import { AppState } from "./state.r.ts";

/**
 * Update application state.
 */
export function update(state: AppState): void {
    // Just to demonstrate: accessing state.tasks
}

/**
 * Get task count.
 */
export function getTaskCount(tasks: Task[]): number {
    return tasks.length;
}
