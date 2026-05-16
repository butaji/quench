//! # Project Initialization
//!
//! Creates new Rune projects with proper structure.

use crate::Result;
use super::BuildDriver;

impl BuildDriver {
    /// Initialize project structure.
    pub fn init_project_structure(&self) -> Result<()> {
        let project_name = &self.config.project.name;
        let base = self.options.workspace.join("crates");

        // Create directory structure
        std::fs::create_dir_all(base.join(&self.config.build.target_crate).join("src/native"))?;
        std::fs::create_dir_all(base.join(&self.config.build.target_crate).join("src/views"))?;
        std::fs::create_dir_all(base.join("protocol/src"))?;
        std::fs::create_dir_all(base.join("host/src"))?;

        // Create workspace Cargo.toml
        let workspace_cargo = format!(r#"[workspace]
resolver = "2"
members = [
    "crates/protocol",
    "crates/host",
    "crates/{}",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
authors = ["{}"]
license = "MIT"
rust-version = "1.75"

[workspace.dependencies]
ratatui = "0.26"
crossterm = "0.27"
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
"#,
            self.config.build.target_crate,
            project_name
        );
        std::fs::write(self.options.workspace.join("Cargo.toml"), workspace_cargo)?;

        // Create rune.toml
        let rune_config = format!(r#"[project]
name = "{}"

[build]
target_crate = "{}"
host_crate = "host"

[dev]
hot_reload = true
debounce = 100

[release]
static_binary = true
lto = true
"#,
            project_name,
            self.config.build.target_crate
        );
        std::fs::write(self.options.workspace.join("rune.toml"), rune_config)?;

        Ok(())
    }

    /// Initialize protocol crate.
    pub fn init_protocol(&self) -> Result<()> {
        let proto_dir = self.options.workspace.join("crates/protocol");

        let cargo = r#"[package]
name = "protocol"
version = "0.1.0"
edition = "2021"

[dependencies]
ratatui = { workspace = true }
crossterm = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
"#;
        std::fs::write(proto_dir.join("Cargo.toml"), cargo)?;

        let lib = "//! # Protocol
//!
//! Shared protocol between host and app dylib.

use serde::{Deserialize, Serialize};

/// Application state - owned by host, shared with app.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppState {
    /// Current tasks
    pub tasks: Vec<Task>,
    /// Currently selected task index
    pub selected: usize,
    /// Filter mode
    pub filter: Filter,
    /// Whether the app should exit
    pub should_exit: bool,
}

/// A task item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique ID
    pub id: i32,
    /// Task title
    pub title: String,
    /// Completion status
    pub done: bool,
}

/// Filter mode for task list.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Active,
    Completed,
}

/// Application trait - implemented by app dylib.
pub trait App {
    /// Update application state.
    fn update(&mut self, state: &mut AppState);

    /// Render the application to a terminal frame.
    fn render(&self, f: &mut ratatui::Frame<'_>, state: &AppState);

    /// Handle key events.
    fn handle_key(&mut self, key: crossterm::event::KeyEvent, state: &mut AppState);
}
";
        std::fs::write(proto_dir.join("src/lib.rs"), lib)?;

        Ok(())
    }

    /// Initialize host crate.
    pub fn init_host(&self) -> Result<()> {
        let host_dir = self.options.workspace.join("crates/host");

        let cargo = format!(r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "{}"
path = "src/main.rs"

[dependencies]
protocol = {{ path = "../protocol" }}
ratatui = {{ workspace = true }}
crossterm = {{ workspace = true }}
serde = {{ workspace = true }}
serde_json = {{ workspace = true }}
libloading = "0.8"
"#,
            self.config.build.host_crate,
            self.config.build.host_crate
        );
        std::fs::write(host_dir.join("Cargo.toml"), cargo)?;

        let main = r#"//! # Host Binary
//!
//! Thin host binary that loads and manages the app dylib.

use std::path::PathBuf;
use libloading::Library;
use protocol::{App, AppState};
use ratatui::{Terminal, backend::CrosstermBackend};

pub struct AppLoader {
    lib: Option<Library>,
    creator: Option<unsafe extern "C" fn() -> *mut dyn App>,
}

impl AppLoader {
    /// Load a new dylib.
    pub unsafe fn load(path: &PathBuf) -> Result<Self, libloading::Error> {
        let lib = Library::new(path)?;
        let creator: libloading::Symbol<
            unsafe extern "C" fn() -> *mut dyn App
        > = lib.get(b"create_app")?;
        Ok(Self {
            lib: Some(lib),
            creator: Some(*creator),
        })
    }

    /// Create a new app instance.
    pub fn create_app(&self) -> Box<dyn App> {
        unsafe {
            let creator = self.creator.unwrap();
            let ptr = creator();
            Box::from_raw(ptr)
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup terminal
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Initial state
    let mut state = AppState::default();
    let mut app_loader: Option<AppLoader> = None;

    // Find hot directory
    let hot_dir = PathBuf::from("target/hot");
    let current_link = hot_dir.join(".current");

    // Main event loop
    loop {
        // Check for new dylib
        if current_link.exists() {
            if let Ok(target) = std::fs::read_link(&current_link) {
                if app_loader.is_none()
                    || app_loader.as_ref().unwrap().lib.as_ref().map(|l| l.path() != Some(target.clone())).unwrap_or(true)
                {
                    unsafe { app_loader = Some(AppLoader::load(&target)?); }
                }
            }
        }

        // Update and render
        if let Some(ref loader) = app_loader {
            let mut app = loader.create_app();
            app.update(&mut state);
            terminal.draw(|f| {
                app.render(f, &state);
            })?;
        }

        // Handle events
        if let Ok(event) = crossterm::event::read() {
            if let crossterm::event::Event::Key(key) = event {
                if key.code == crossterm::event::KeyCode::Char('q') {
                    break;
                }
                if let Some(ref loader) = app_loader {
                    let mut app = loader.create_app();
                    app.handle_key(key, &mut state);
                }
            }
        }

        if state.should_exit {
            break;
        }
    }

    // Cleanup
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen
    )?;
    crossterm::terminal::disable_raw_mode()?;

    Ok(())
}
"#;
        std::fs::write(host_dir.join("src/main.rs"), main)?;

        Ok(())
    }

    /// Initialize app crate.
    pub fn init_app(&self) -> Result<()> {
        let app_dir = self.options.workspace.join("crates").join(&self.config.build.target_crate);

        let cargo = format!(r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[lib]
name = "{}"
crate-type = ["cdylib", "rlib"]
path = "src/lib.rs"

[dependencies]
protocol = {{ path = "../protocol" }}
ratatui = {{ workspace = true }}
crossterm = {{ workspace = true }}
serde = {{ workspace = true }}
serde_json = {{ workspace = true }}

[build-dependencies]
rune = {{ path = "../../.." }}
"#,
            self.config.build.target_crate,
            self.config.build.target_crate
        );
        std::fs::write(app_dir.join("Cargo.toml"), cargo)?;

        // lib.rs
        let lib = r#"//! # App Library
//!
//! Hot-reloadable application logic.

mod native;

use protocol::{App, AppState, Filter};

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
        let widget = generated::root::render(state);
        f.render_widget(widget, f.size());
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent, state: &mut AppState) {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if state.selected < state.tasks.len().saturating_sub(1) {
                    state.selected += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if state.selected > 0 {
                    state.selected -= 1;
                }
            }
            KeyCode::Char('x') => {
                if let Some(task) = state.tasks.get_mut(state.selected) {
                    task.done = !task.done;
                }
            }
            KeyCode::Char('q') => {
                state.should_exit = true;
            }
            _ => {}
        }
    }
}
"#;
        std::fs::write(app_dir.join("src/lib.rs"), lib)?;

        // Native module
        let native_mod = "pub mod fast_math;\n";
        std::fs::create_dir_all(app_dir.join("src/native"))?;
        std::fs::write(app_dir.join("src/native/mod.rs"), native_mod)?;

        let fast_math = "//! Fast math utilities written in Rust.

/// Fast square root approximation.
pub fn fast_sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// Fast sine approximation using polynomial.
pub fn fast_sin(x: f64) -> f64 {
    x.sin()
}
";
        std::fs::write(app_dir.join("src/native/fast_math.rs"), fast_math)?;

        // Main Rune file
        let main_ts = r#"// main.r.ts - Main entry point for the app logic

import { AppState, Task, Filter } from "./state.r.ts";

/**
 * Update application state.
 */
export function update(state: AppState): void {
    if (state.selected >= state.tasks.length) {
        state.selected = Math.max(0, state.tasks.length - 1);
    }
}
"#;
        std::fs::write(app_dir.join("src/main.r.ts"), main_ts)?;

        // State Rune file
        let state_ts = r#"// state.r.ts - Application state types

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
        std::fs::write(app_dir.join("src/state.r.ts"), state_ts)?;

        // Root view (TSX)
        let root_tsx = r#"// root.r.tsx - Main view component

import { AppState, Filter } from "../state.r.ts";

export function render(state: AppState): Paragraph {
    const title = `TODOX - ${state.tasks.length} tasks`;
    let content = title + "\n";
    content += "-".repeat(40) + "\n";
    for (let i = 0; i < state.tasks.length; i++) {
        const task = state.tasks[i];
        const marker = task.done ? "[x]" : "[ ]";
        const prefix = i === state.selected ? "> " : "  ";
        content += `${prefix}${marker} ${task.title}\n`;
    }
    content += "\nPress q to quit, j/k to navigate, x to toggle";
    return Paragraph::new(content);
}
"#;
        std::fs::create_dir_all(app_dir.join("src/views"))?;
        std::fs::write(app_dir.join("src/views/root.r.tsx"), root_tsx)?;

        // Task list view
        let task_list_tsx = r#"// task_list.r.tsx - Task list component

import { Task, Filter } from "../state.r.ts";

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
"#;
        std::fs::write(app_dir.join("src/views/task_list.r.tsx"), task_list_tsx)?;

        Ok(())
    }
}
