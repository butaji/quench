// main.r.ts - TodoX Application Logic
//
// Demonstrates the Rune TypeScript subset:
// - Struct types (Task, AppState)
// - Tagged union enums (Filter)
// - Option pattern (Task | null)
// - Result pattern ({ok, value} / {ok, error})
// - Array operations
// - Native interop via native: imports

/// Task structure with id, title, and completion status.
export type Task = {
    id: number;
    title: string;
    done: boolean;
};

/// Filter enum using tagged union pattern.
export enum Filter {
    All = "all",
    Active = "active",
    Completed = "completed",
}

/// Application state - the state owner in host.
export type AppState = {
    tasks: Task[];
    selected: number;
    filter: Filter;
    shouldExit: boolean;
};

/// Create a new task with a unique id.
export function createTask(title: string): Task {
    return {
        id: Math.floor(Math.random() * 1000000),
        title,
        done: false,
    };
}

/// Toggle task completion.
export function toggleTask(task: Task): Task {
    return { ...task, done: !task.done };
}

/// Filter tasks by status.
export function filterTasks(tasks: Task[], filter: Filter): Task[] {
    switch (filter) {
        case Filter.Active:
            return tasks.filter(t => !t.done);
        case Filter.Completed:
            return tasks.filter(t => t.done);
        default:
            return tasks;
    }
}

/// Find task by id - Option pattern.
export function findTask(tasks: Task[], id: number): Task | null {
    for (let i = 0; i < tasks.length; i++) {
        if (tasks[i].id === id) {
            return tasks[i];
        }
    }
    return null;
}

/// Validate task title - Result pattern.
export function validateTitle(title: string): 
    | { ok: true, value: string }
    | { ok: false, error: string }
{
    if (title.length === 0) {
        return { ok: false, error: "Title cannot be empty" };
    }
    if (title.length > 100) {
        return { ok: false, error: "Title too long (max 100 chars)" };
    }
    return { ok: true, value: title };
}

/// Add a task to the list.
export function addTask(state: AppState, title: string): AppState {
    const validation = validateTitle(title);
    if (!validation.ok) {
        return state; // Validation failed, return unchanged
    }

    const newTask = createTask(validation.value);
    return {
        ...state,
        tasks: [...state.tasks, newTask],
        selected: state.tasks.length,
    };
}

/// Toggle task by index.
export function toggleTaskByIndex(state: AppState, index: number): AppState {
    if (index < 0 || index >= state.tasks.length) {
        return state;
    }

    const updatedTasks = state.tasks.map((task, i) => 
        i === index ? toggleTask(task) : task
    );

    return { ...state, tasks: updatedTasks };
}

/// Delete task by index.
export function deleteTaskByIndex(state: AppState, index: number): AppState {
    if (index < 0 || index >= state.tasks.length) {
        return state;
    }

    const filteredTasks = state.tasks.filter((_, i) => i !== index);
    let newSelected = state.selected;
    if (newSelected >= filteredTasks.length && filteredTasks.length > 0) {
        newSelected = filteredTasks.length - 1;
    }

    return {
        ...state,
        tasks: filteredTasks,
        selected: newSelected,
    };
}

/// Change the filter.
export function changeFilter(state: AppState, filter: Filter): AppState {
    return { ...state, filter };
}

/// Navigation helpers.
export function moveSelection(state: AppState, delta: number): AppState {
    const filteredTasks = filterTasks(state.tasks, state.filter);
    if (filteredTasks.length === 0) {
        return state;
    }

    let newSelected = state.selected + delta;
    newSelected = Math.max(0, Math.min(newSelected, filteredTasks.length - 1));

    return { ...state, selected: newSelected };
}

/// Count tasks by status.
export function countTasks(tasks: Task[]): {
    total: number;
    active: number;
    completed: number;
} {
    let active = 0;
    let completed = 0;

    for (let i = 0; i < tasks.length; i++) {
        if (tasks[i].done) {
            completed++;
        } else {
            active++;
        }
    }

    return {
        total: tasks.length,
        active,
        completed,
    };
}

// Native interop - math functions from Rust
// import { fastSqrt, fibonacci } from "native:math";

// Export filter enum values for JSX views
export const FILTER_ALL = Filter.All;
export const FILTER_ACTIVE = Filter.Active;
export const FILTER_COMPLETED = Filter.Completed;
