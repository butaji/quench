// handlers/keyboard.r.ts - Keyboard event handling.
// Demonstrates: tagged unions, message passing pattern, native interop

import { AppState, Filter } from "../state.r.ts";

/// Keyboard message type - tagged union pattern.
export type KeyboardMessage =
    | { tag: "Move"; dx: number; dy: number }
    | { tag: "Quit" }
    | { tag: "Write"; text: string }
    | { tag: "Toggle" }
    | { tag: "Add" }
    | { tag: "Delete" }
    | { tag: "Filter" };

/// Handle keyboard message and update state.
export function handleMessage(msg: KeyboardMessage, state: AppState): void {
    switch (msg.tag) {
        case "Move":
            handleMove(state, msg.dx, msg.dy);
            break;
        case "Quit":
            state.shouldExit = true;
            break;
        case "Write":
            handleWrite(state, msg.text);
            break;
        case "Toggle":
            handleToggle(state);
            break;
        case "Add":
            handleAdd(state);
            break;
        case "Delete":
            handleDelete(state);
            break;
        case "Filter":
            handleFilter(state);
            break;
    }
}

/// Handle cursor movement.
function handleMove(state: AppState, dx: number, dy: number): void {
    const newIdx = state.selected + dx + dy;
    if (newIdx >= 0 && newIdx < state.tasks.length) {
        state.selected = newIdx;
    }
}

/// Handle text input (placeholder).
function handleWrite(state: AppState, text: string): void {
    // Would be used for editing task titles
    // Placeholder for future implementation
}

/// Handle task toggle.
function handleToggle(state: AppState): void {
    if (state.tasks.length > 0 && state.selected < state.tasks.length) {
        const task = state.tasks[state.selected];
        task.done = !task.done;
    }
}

/// Handle adding a new task.
function handleAdd(state: AppState): void {
    // Placeholder - would open input mode
}

/// Handle deleting current task.
function handleDelete(state: AppState): void {
    if (state.tasks.length > 0) {
        state.tasks.splice(state.selected, 1);
        if (state.selected >= state.tasks.length && state.tasks.length > 0) {
            state.selected = state.tasks.length - 1;
        }
    }
}

/// Cycle through filters.
function handleFilter(state: AppState): void {
    if (state.filter === Filter.All) {
        state.filter = Filter.Active;
    } else if (state.filter === Filter.Active) {
        state.filter = Filter.Completed;
    } else {
        state.filter = Filter.All;
    }
}
