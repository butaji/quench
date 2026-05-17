// keyboard.r.ts - Keyboard event handler
//
// Handles keyboard input for navigation and task management.

import { AppState, Filter, Task, toggle_task, create_task } from "../state.r.ts";
import { batch_toggle_by_id } from "native:fast_math";

/// Message type for keyboard events.
export type Message =
    | { tag: "Navigate", direction: number }
    | { tag: "Toggle" }
    | { tag: "Delete" }
    | { tag: "NewTask", title: string }
    | { tag: "Filter", filter: Filter }
    | { tag: "Quit" };

/// Handle a keyboard event.
export function handle_message(msg: Message, state: AppState): void {
    switch (msg.tag) {
        case "Navigate":
            state.selected = Math.max(0, Math.min(
                state.tasks.length - 1,
                state.selected + msg.direction
            ));
            break;
        case "Toggle":
            if (state.selected < state.tasks.length) {
                state.tasks[state.selected] = toggle_task(state.tasks[state.selected]);
            }
            break;
        case "Delete":
            if (state.selected < state.tasks.length) {
                state.tasks.splice(state.selected, 1);
                if (state.selected >= state.tasks.length && state.selected > 0) {
                    state.selected--;
                }
            }
            break;
        case "NewTask":
            state.tasks.push(create_task(msg.title));
            state.selected = state.tasks.length - 1;
            break;
        case "Filter":
            state.filter = msg.filter;
            break;
        case "Quit":
            state.shouldExit = true;
            break;
    }
}

/// Handle key event from host.
export function handle_key(
    key: { code: string, char: string },
    state: AppState
): void {
    switch (key.code) {
        case "j":
        case "ArrowDown":
            state.selected = Math.min(state.selected + 1, state.tasks.length - 1);
            break;
        case "k":
        case "ArrowUp":
            state.selected = Math.max(0, state.selected - 1);
            break;
        case "x":
        case " ":
            if (state.selected < state.tasks.length) {
                state.tasks[state.selected] = toggle_task(state.tasks[state.selected]);
            }
            break;
        case "d":
            if (state.selected < state.tasks.length) {
                state.tasks.splice(state.selected, 1);
            }
            break;
        case "a":
        case "i":
        case "o":
            const title = `New task ${state.tasks.length + 1}`;
            state.tasks.push(create_task(title));
            state.selected = state.tasks.length - 1;
            break;
        case "q":
            state.shouldExit = true;
            break;
        case "f":
            // Cycle filter
            const filters: Filter[] = [Filter.All, Filter.Active, Filter.Completed];
            const idx = filters.indexOf(state.filter);
            state.filter = filters[(idx + 1) % filters.length];
            break;
    }
}
