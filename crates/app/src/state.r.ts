//! state.r.ts - Application state types

/// Types are defined in the protocol crate.
/// This file provides helpers that operate on them.

import { Task, Filter } from "protocol";

export function createTask(title: string): Task {
    return {
        id: Date.now(),
        title,
        done: false,
    };
}

export function toggleTask(task: Task): Task {
    return { ...task, done: !task.done };
}
