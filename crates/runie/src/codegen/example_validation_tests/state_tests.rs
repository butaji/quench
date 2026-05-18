//! # Example State Tests
//!
//! Tests for state management and model examples.

use crate::{analyzer, codegen, parser};

/// Test todox example with state management.
#[test]
fn test_example_todox_state() {
    let source = "
export type Task = {
    id: number,
    title: string,
    done: boolean,
};

export enum Filter {
    All = \"all\",
    Active = \"active\",
    Completed = \"completed\",
}

export type AppState = {
    tasks: Task[],
    selected: number,
    filter: Filter,
    shouldExit: boolean,
};

export function createTask(title: string): Task {
    return {
        id: Date.now(),
        title: title,
        done: false,
    };
}

export function toggleTask(task: Task): Task {
    return { ...task, done: !task.done };
}

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
";
    let file = parser::parse_file_from_str(source, "state.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
    assert!(result.source.contains("Task"));
    assert!(result.source.contains("Filter"));
    assert!(result.source.contains("AppState"));
    assert!(result.source.contains("create_task"));
    assert!(result.source.contains("toggle_task"));
    assert!(result.source.contains("filter_tasks"));
}

/// Test UI demo with counter and state.
#[test]
fn test_example_ui_demo() {
    let source = "
export type State = {
    counter: number,
    items: string[],
    selectedIndex: number,
    inputBuffer: string,
};

export function createInitialState(): State {
    return {
        counter: 0,
        items: [\"Learn Rust\", \"Build UI\", \"Ship product\"],
        selectedIndex: 0,
        inputBuffer: \"\",
    };
}

export function incrementCounter(state: State): void {
    state.counter = state.counter + 1;
}

export function addItem(state: State, item: string): void {
    state.items.push(item);
}

export function removeItem(state: State, index: number): void {
    if (index >= 0 && index < state.items.length) {
        state.items.splice(index, 1);
    }
}
";
    let file = parser::parse_file_from_str(source, "ui_demo.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
    assert!(result.source.contains("State"));
    assert!(result.source.contains("create_initial_state"));
    assert!(result.source.contains("increment_counter"));
    assert!(result.source.contains("add_item"));
    assert!(result.source.contains("remove_item"));
}

/// Test data processing with generics.
#[test]
fn test_example_data_processing() {
    let source = "
export type Person = {
    id: number,
    name: string,
    age: number,
    salary: number,
};

export type Filter = {
    minAge?: number,
    maxAge?: number,
    minSalary?: number,
};

export function first<T>(arr: T[]): T | null {
    if (arr.length > 0) {
        return arr[0];
    }
    return null;
}

export function filterBy<T>(
    items: T[],
    predicate: (item: T) => boolean
): T[] {
    const result: T[] = [];
    for (const item of items) {
        if (predicate(item)) {
            result.push(item);
        }
    }
    return result;
}

export function mapItems<T, U>(
    items: T[],
    transform: (item: T) => U
): U[] {
    const result: U[] = [];
    for (const item of items) {
        result.push(transform(item));
    }
    return result;
}
";
    let file = parser::parse_file_from_str(source, "data.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
    assert!(result.source.contains("Person"));
    assert!(result.source.contains("first"));
    assert!(result.source.contains("filter_by"));
    assert!(result.source.contains("map_items"));
}

/// Test ratatui UI patterns.
#[test]
fn test_example_ratatui_ui() {
    let source = "
export type Item = {
    id: number,
    name: string,
    price: number,
    quantity: number,
};

export type ViewState =
    | { tag: \"List\" }
    | { tag: \"Add\" }
    | { tag: \"Edit\"; itemId: number };

export type AppState = {
    items: Item[],
    selected: number,
    view: ViewState,
    total: number,
};

export function getTotalValue(state: AppState): number {
    let total = 0;
    for (const item of state.items) {
        total = total + item.price * item.quantity;
    }
    return total;
}

export function createItem(name: string, price: number): Item {
    return {
        id: Date.now(),
        name: name,
        price: price,
        quantity: 1,
    };
}

export function filterByName(items: Item[], query: string): Item[] {
    const q = query.toLowerCase();
    return items.filter(item => item.name.toLowerCase().includes(q));
}
";
    let file = parser::parse_file_from_str(source, "ratatui.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
    assert!(result.source.contains("Item"));
    assert!(result.source.contains("ViewState"));
    assert!(result.source.contains("AppState"));
    assert!(result.source.contains("get_total_value"));
    assert!(result.source.contains("create_item"));
    assert!(result.source.contains("filter_by_name"));
}

/// Test async HTTP patterns.
#[test]
fn test_example_async_http() {
    let source = "
export type HttpResponse<T> = {
    ok: boolean,
    status: number,
    data?: T,
    error?: string,
};

export type User = {
    id: number,
    name: string,
    email: string,
};

export type Post = {
    id: number,
    userId: number,
    title: string,
};

export async function fetchUser(id: number): Promise<User | null> {
    const response = await httpGet(`/users/${id}`);
    return response.data;
}

export async function fetchPosts(userId: number): Promise<Post[]> {
    const response = await httpGet(`/posts?userId=${userId}`);
    return response.data ?? [];
}

export function validateEmail(email: string): boolean {
    return email.includes(\"@\");
}
";
    let file = parser::parse_file_from_str(source, "http.r.ts").unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    assert!(!result.source.is_empty());
    assert!(result.source.contains("HttpResponse"));
    assert!(result.source.contains("User"));
    assert!(result.source.contains("Post"));
    assert!(result.source.contains("fetch_user"));
    assert!(result.source.contains("validate_email"));
}
