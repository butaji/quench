//! # App Templates
//!
//! Templates for the app crate.

/// App lib.rs template.
pub const APP_LIB: &str = r#"//! # App Library
//!
//! Hot-reloadable application logic.

mod native;

use protocol::{App, AppState};

/// Create a new app instance.
#[no_mangle]
pub extern "C" fn create_app() -> *mut dyn App {
    Box::into_raw(Box::new(AppImpl::default()))
}

#[derive(Default)]
struct AppImpl;

impl App for AppImpl {
    fn update(&mut self, state: &mut AppState) {
        generated::main::update(state);
    }

    fn render(&self, f: &mut ratatui::Frame<'_>, state: &AppState) {
        generated::views::root::render(f, state);
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent, state: &mut AppState) {
        generated::main::handle_key(key, state);
    }
}
"#;

/// Main.r.ts template.
pub const MAIN_RS: &str = r#"//! main.r.ts - Main entry point

import { AppState, Task, Filter } from "./state.r.ts";

/// Update application state.
export function update(state: AppState): void {
    if (state.selected >= state.tasks.length) {
        state.selected = Math.max(0, state.tasks.length - 1);
    }
}

/// Handle key events.
export function handleKey(key: KeyEvent, state: AppState): void {
    if (key === "j") {
        state.selected = Math.min(state.selected + 1, state.tasks.length - 1);
    } else if (key === "k") {
        state.selected = Math.max(state.selected - 1, 0);
    } else if (key === "x") {
        const task = state.tasks[state.selected];
        if (task) {
            task.done = !task.done;
        }
    }
}
"#;

/// State.r.ts template.
pub const STATE_RS: &str = r#"//! state.r.ts - Application state types

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
"#;
