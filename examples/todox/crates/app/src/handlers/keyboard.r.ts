// keyboard.r.ts - Keyboard event handlers

import { AppState, Task } from "../state.r.ts";

/**
 * Keyboard key codes.
 */
export enum KeyCode {
    Up = "ArrowUp",
    Down = "ArrowDown",
    Left = "ArrowLeft",
    Right = "ArrowRight",
    Enter = "Enter",
    Space = " ",
    Escape = "Escape",
    Tab = "Tab",
}

/**
 * Handle navigation keys.
 */
export function handleNavigation(key: KeyCode, state: AppState): void {
    switch (key) {
        case KeyCode.Up:
        case "k":
            if (state.selected > 0) {
                state.selected -= 1;
            }
            break;
        case KeyCode.Down:
        case "j":
            if (state.selected < state.tasks.length - 1) {
                state.selected += 1;
            }
            break;
        case KeyCode.Home:
            state.selected = 0;
            break;
        case KeyCode.End:
            state.selected = Math.max(0, state.tasks.length - 1);
            break;
    }
}

/**
 * Handle task-related keys.
 */
export function handleTaskAction(key: KeyCode, state: AppState): void {
    switch (key) {
        case KeyCode.Enter:
        case KeyCode.Space:
            toggleSelectedTask(state);
            break;
        case "d":
            deleteSelectedTask(state);
            break;
        case "a":
            addNewTask(state);
            break;
    }
}

/**
 * Toggle the currently selected task.
 */
export function toggleSelectedTask(state: AppState): void {
    const task = state.tasks[state.selected];
    if (task) {
        task.done = !task.done;
    }
}

/**
 * Delete the currently selected task.
 */
export function deleteSelectedTask(state: AppState): void {
    state.tasks.splice(state.selected, 1);
    if (state.selected >= state.tasks.length && state.tasks.length > 0) {
        state.selected = state.tasks.length - 1;
    }
}

/**
 * Add a new task.
 */
export function addNewTask(state: AppState): void {
    const task: Task = {
        id: Date.now(),
        title: "New Task",
        done: false,
    };
    state.tasks.push(task);
    state.selected = state.tasks.length - 1;
}
