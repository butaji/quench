// state.r.ts - Application state types
// These types are imported from protocol crate via lib.rs re-exports

/**
 * Create a new task with the given title.
 */
export function createTask(title: string): Task {
    return {
        id: Date.now(),
        title,
        done: false,
    };
}

/**
 * Toggle the completion status of a task.
 */
export function toggleTask(task: Task): Task {
    return { ...task, done: !task.done };
}

/**
 * Filter tasks by the given filter mode.
 */
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
