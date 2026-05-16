// main.r.ts - Main entry point for the app logic

import { Task, AppState } from "./state.r.ts";

/**
 * Application update function.
 */
export function update(_state: AppState): void {
    // Main update logic handled by host event loop
}

/**
 * Get task count for display.
 */
export function getTaskCount(tasks: Task[]): number {
    return tasks.length;
}

/**
 * Check if any task is done using array.some().
 */
export function hasCompletedTasks(tasks: Task[]): boolean {
    return tasks.some((task) => task.done);
}
