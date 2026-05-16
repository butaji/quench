// state.r.ts - Application state types

/**
 * A task item.
 */
export type Task = {
    id: number,
    title: string,
    done: boolean,
};

/**
 * Filter mode for task list.
 */
export enum Filter {
    All = "All",
    Active = "Active",
    Completed = "Completed",
}

/**
 * Application state owned by host.
 */
export type AppState = {
    tasks: Task[],
    selected: number,
    filter: Filter,
    should_exit: boolean,
};
