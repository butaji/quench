// src/main.r.ts - Entry point
// Demonstrates: Result patterns, error handling, key events

import { AppState, createItem, filterByName } from "./state.r.ts";

export type KeyEvent = {
    code: string;
    char?: string;
};

export enum KeyCode {
    Char = "char",
    Enter = "enter",
    Escape = "esc",
    Up = "up",
    Down = "down",
}

// Validate price input
export function validatePrice(price: number): 
    | { ok: true, value: number }
    | { ok: false, error: string }
{
    if (price < 0) {
        return { ok: false, error: "Price cannot be negative" };
    }
    if (price > 1000000) {
        return { ok: false, error: "Price exceeds maximum" };
    }
    return { ok: true, value: price };
}

// Find item by id
export function findById(items: Item[], id: number): Item | null {
    for (let i = 0; i < items.length; i++) {
        if (items[i].id === id) {
            return items[i];
        }
    }
    return null;
}

// Update app state
export function update(state: AppState): void {
    // Ensure selection is in bounds
    if (state.items.length === 0) {
        state.selected = 0;
    } else if (state.selected >= state.items.length) {
        state.selected = state.items.length - 1;
    }
}

// Handle key press
export function handleKey(key: KeyEvent, state: AppState): void {
    switch (key.code) {
        case KeyCode.Down:
        case KeyCode.Char:
            if (key.char === "j") {
                state.selected = Math.min(state.selected + 1, state.items.length - 1);
            }
            break;
        case KeyCode.Up:
        case KeyCode.Char:
            if (key.char === "k") {
                state.selected = Math.max(state.selected - 1, 0);
            }
            break;
        case KeyCode.Char:
            if (key.char === "a") {
                const item = createItem("New Item", 9.99);
                state.items.push(item);
                state.selected = state.items.length - 1;
            } else if (key.char === "d") {
                if (state.items.length > 0) {
                    const item = state.items[state.selected];
                    if (item) {
                        state.view = { tag: "Detail", id: item.id };
                    }
                }
            } else if (key.char === "e") {
                if (state.items.length > 0) {
                    const item = state.items[state.selected];
                    if (item) {
                        state.view = { tag: "Edit", id: item.id };
                    }
                }
            } else if (key.char === "l") {
                state.view = { tag: "List" };
            } else if (key.char === "q") {
                state.shouldExit = true;
            }
            break;
        case KeyCode.Escape:
            state.view = { tag: "List" };
            break;
    }
}
