//! main.r.ts - Main entry point

import { state } from "rune/hot";
import { AppState } from "protocol";

/// Update application state.
export function update(): void {
    const s = state();

    // Clamp selection to valid range
    if (s.selected >= s.tasks.length && s.tasks.length > 0) {
        s.selected = s.tasks.length - 1;
    }
}
