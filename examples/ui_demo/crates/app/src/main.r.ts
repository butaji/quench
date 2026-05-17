// main.r.ts - Ratatui UI demonstration.
// 
// Demonstrates:
// - Multiple views
// - Interactive state
// - Styled widgets

/// Application state.
export type State = {
    counter: number;
    items: string[];
    selectedIndex: number;
    inputBuffer: string;
};

/// Create initial state.
export function createInitialState(): State {
    return {
        counter: 0,
        items: ["Learn Rust", "Build UI", "Ship product"],
        selectedIndex: 0,
        inputBuffer: "",
    };
}

/// Increment counter.
export function incrementCounter(state: State): void {
    state.counter = state.counter + 1;
}

/// Decrement counter.
export function decrementCounter(state: State): void {
    state.counter = state.counter - 1;
}

/// Move selection up.
export function moveSelectionUp(state: State): void {
    if (state.selectedIndex > 0) {
        state.selectedIndex = state.selectedIndex - 1;
    }
}

/// Move selection down.
export function moveSelectionDown(state: State): void {
    if (state.selectedIndex < state.items.length - 1) {
        state.selectedIndex = state.selectedIndex + 1;
    }
}

/// Add new item.
export function addItem(state: State, item: string): void {
    state.items.push(item);
    state.selectedIndex = state.items.length - 1;
}

/// Remove selected item.
export function removeSelected(state: State): void {
    if (state.items.length > 0 && state.selectedIndex < state.items.length) {
        state.items.splice(state.selectedIndex, 1);
        if (state.selectedIndex >= state.items.length && state.items.length > 0) {
            state.selectedIndex = state.items.length - 1;
        }
    }
}

/// Toggle item completion (mark with prefix).
export function toggleItem(state: State): void {
    // In a real app, this would modify item state
    // For demo, we just cycle selection
    moveSelectionDown(state);
}
