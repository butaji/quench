//! main.r.ts - Main entry point

import { AppState } from "protocol";

/// Update application state.
export function update(state: AppState): void {
    // State validation only - mutations handled by host
    const _ = state.tasks.length;
}
