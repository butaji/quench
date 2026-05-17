//! state.r.ts - Application state types
//!
//! Type definitions for the todo app state.

import { fastSqrt } from "native:fast_math";

/// A task item.
export type Task = {
    id: number;
    title: string;
    done: boolean;
};

/// Filter mode for task list.
export enum Filter {
    All = "all",
    Active = "active",
    Completed = "completed",
}

/// Application state.
export type AppState = {
    tasks: Task[];
    selected: number;
    filter: Filter;
    shouldExit: boolean;
};

/// Create a new task with generated ID.
export function create_task(title: string): Task {
    const id = Math.floor(Date.now() / 1000);
    return {
        id,
        title,
        done: false,
    };
}

/// Toggle a task's completion status.
export function toggle_task(task: Task): Task {
    return { ...task, done: !task.done };
}

/// Calculate importance score using native Rust function.
export function task_importance(task: Task): number {
    const base_score = task.title.length;
    return fastSqrt(base_score);
}
