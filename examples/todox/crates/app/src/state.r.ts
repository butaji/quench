// state.r.ts - Application state types and functions.

export type Task = {
    id: number;
    title: string;
    done: boolean;
};

export enum Filter {
    All = "all",
    Active = "active",
    Completed = "completed",
}

export type AppState = {
    tasks: Task[];
    selected: number;
    filter: Filter;
    shouldExit: boolean;
};

/// Create a new task.
export function createTask(title: string): Task {
    return {
        id: Date.now(),
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
