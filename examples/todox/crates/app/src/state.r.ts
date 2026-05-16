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
 * Application state owned by host.
 */
export type AppState = {
    tasks: Task[],
    selected: number,
};
