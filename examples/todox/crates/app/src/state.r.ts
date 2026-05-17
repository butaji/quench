// state.r.ts - Application state types and functions.
// Demonstrates: structs, enums, Option, arrays, Result pattern

/// Task structure with id, title, and completion status.
export type Task = {
    id: number;
    title: string;
    done: boolean;
};

/// Filter enum for showing active/completed/all tasks.
export enum Filter {
    All = 0,
    Active = 1,
    Completed = 2,
}

/// Application state owned by the host.
export type AppState = {
    tasks: Task[];
    selected: number;
    filter: Filter;
    shouldExit: boolean;
};

/// Create a new task with generated id.
export function createTask(title: string): Task {
    return {
        id: Math.floor(Math.random() * 1000000),
        title,
        done: false,
    };
}

/// Toggle task completion status.
export function toggleTask(task: Task): Task {
    return { ...task, done: !task.done };
}

/// Filter tasks by status using tagged union pattern.
export function filterTasks(tasks: Task[], filter: Filter): Task[] {
    switch (filter) {
        case Filter.Active:
            return tasks.filter(t => !t.done);
        case Filter.Completed:
            return tasks.filter(t => t.done);
        default:
            return [...tasks];
    }
}

/// Find task by id - returns Option pattern.
export function findTask(tasks: Task[], id: number): Task | null {
    for (let i = 0; i < tasks.length; i++) {
        if (tasks[i].id === id) {
            return tasks[i];
        }
    }
    return null;
}

/// Get completed task count.
export function getCompletedCount(tasks: Task[]): number {
    let count = 0;
    for (let i = 0; i < tasks.length; i++) {
        if (tasks[i].done) {
            count++;
        }
    }
    return count;
}
