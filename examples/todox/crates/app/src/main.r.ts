// main.r.ts - Main entry point for app logic.
// Demonstrates: tagged unions, Option pattern, native interop, key handling

import { AppState, Filter, Task, createTask, toggleTask } from "./state.r.ts";
import { handleKeyNative } from "native:handlers";

/// Key codes matching crossterm KeyCode
export enum KeyCode {
    Char = "char",
    Enter = "enter",
    Escape = "esc",
    Left = "left",
    Right = "right",
    Up = "up",
    Down = "down",
}

/// Key event structure
export type KeyEvent = {
    code: KeyCode;
    char?: string;
};

/// Update application state - ensures selection is in bounds.
export function update(state: AppState): void {
    if (state.tasks.length === 0) {
        state.selected = 0;
        return;
    }
    if (state.selected >= state.tasks.length) {
        state.selected = state.tasks.length - 1;
    }
}

/// Handle key events for navigation and task manipulation.
export function handleKey(key: KeyEvent, state: AppState): void {
    switch (key.code) {
        case KeyCode.Down:
            state.selected = Math.min(state.selected + 1, state.tasks.length - 1);
            break;
        case KeyCode.Up:
            state.selected = Math.max(state.selected - 1, 0);
            break;
        case KeyCode.Char:
            if (key.char === "j") {
                state.selected = Math.min(state.selected + 1, state.tasks.length - 1);
            } else if (key.char === "k") {
                state.selected = Math.max(state.selected - 1, 0);
            } else if (key.char === "x") {
                const task = state.tasks[state.selected];
                if (task !== null) {
                    state.tasks[state.selected] = toggleTask(task);
                }
            } else if (key.char === "a") {
                const newTask = createTask("New task");
                state.tasks.push(newTask);
                state.selected = state.tasks.length - 1;
            } else if (key.char === "d") {
                if (state.tasks.length > 0) {
                    state.tasks.splice(state.selected, 1);
                    if (state.selected >= state.tasks.length && state.tasks.length > 0) {
                        state.selected = state.tasks.length - 1;
                    }
                }
            } else if (key.char === "f") {
                // Cycle filter
                if (state.filter === Filter.All) {
                    state.filter = Filter.Active;
                } else if (state.filter === Filter.Active) {
                    state.filter = Filter.Completed;
                } else {
                    state.filter = Filter.All;
                }
            }
            break;
    }
}
