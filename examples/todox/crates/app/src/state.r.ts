// state.r.ts - Application state types

/**
 * A task item.
 */
export type Task = {
    id: number;
    title: string;
    done: boolean;
};

/**
 * Filter mode for task list.
 */
export enum Filter {
    All = "all",
    Active = "active",
    Completed = "completed",
}

/**
 * Application state owned by host.
 */
export type AppState = {
    tasks: Task[];
    selected: number;
    filter: Filter;
    shouldExit: boolean;
};

/**
 * Message types for the app.
 */
export type Message =
    | { tag: "AddTask", title: string }
    | { tag: "ToggleTask", id: number }
    | { tag: "DeleteTask", id: number }
    | { tag: "SelectTask", index: number }
    | { tag: "SetFilter", filter: Filter }
    | { tag: "Quit" };

/**
 * Result type for operations.
 */
export type Result<T, E = string> =
    | { ok: true, value: T }
    | { ok: false, error: E };

/**
 * Create a new task.
 */
export function createTask(title: string): Task {
    return {
        id: Date.now(),
        title,
        done: false,
    };
}

/**
 * Toggle task completion.
 */
export function toggleTask(task: Task): Task {
    return { ...task, done: !task.done };
}

/**
 * Validate task title.
 */
export function validateTitle(title: string): Result<string> {
    const trimmed = title.trim();
    if (trimmed.length === 0) {
        return { ok: false, error: "Title cannot be empty" };
    }
    if (trimmed.length > 100) {
        return { ok: false, error: "Title too long (max 100 chars)" };
    }
    return { ok: true, value: trimmed };
}
