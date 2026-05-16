//! # Project Templates
//!
//! Templates for generated project files.

/// Workspace Cargo.toml template.
pub const WORKSPACE_CARGO: &str = "[workspace]
resolver = \"2\"
members = [
    \"crates/protocol\",
    \"crates/host\",
    \"crates/{target_crate}\",
]

[workspace.package]
version = \"0.1.0\"
edition = \"2021\"
authors = [\"{authors}\"]
license = \"MIT\"
rust-version = \"1.75\"

[workspace.dependencies]
ratatui = \"0.26\"
crossterm = \"0.27\"
serde = {{ version = \"1\", features = [\"derive\"] }}
serde_json = \"1\"
";

/// Rune config template.
pub const RUNE_CONFIG: &str = "[project]
name = \"{name}\"

[build]
target_crate = \"{target_crate}\"
host_crate = \"host\"

[dev]
hot_reload = true
debounce = 100

[release]
static_binary = true
lto = true
";

/// Protocol crate Cargo.toml template.
pub const PROTOCOL_CARGO: &str = "[package]
name = \"protocol\"
version = \"0.1.0\"
edition = \"2021\"

[dependencies]
ratatui = { workspace = true }
crossterm = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
";

/// Protocol lib.rs template.
pub const PROTOCOL_LIB: &str = "//! # Protocol
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

/// Host crate Cargo.toml template.
pub const HOST_CARGO: &str = "[package]
name = \"{name}\"
version = \"0.1.0\"
edition = \"2021\"

[[bin]]
name = \"{name}\"
path = \"src/main.rs\"

[dependencies]
protocol = {{ path = \"../protocol\" }}
ratatui = {{ workspace = true }}
crossterm = {{ workspace = true }}
serde = {{ workspace = true }}
serde_json = {{ workspace = true }}
libloading = \"0.8\"
";

/// Host main.rs template.
pub const HOST_MAIN: &str = "//! # Host Binary
//!
//! Thin host binary that loads and manages the app dylib.

use std::path::PathBuf;
use libloading::Library;
use protocol::{App, AppState};
use ratatui::{Terminal, backend::CrosstermBackend};

pub struct AppLoader {
    lib: Option<Library>,
    creator: Option<unsafe extern \"C\" fn() -> *mut dyn App>,
}

impl AppLoader {
    /// Load a new dylib.
    pub unsafe fn load(path: &PathBuf) -> Result<Self, libloading::Error> {
        let lib = Library::new(path)?;
        let creator: libloading::Symbol<
            unsafe extern \"C\" fn() -> *mut dyn App
        > = lib.get(b\"create_app\")?;
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
    let hot_dir = PathBuf::from(\"target/hot\");
    let current_link = hot_dir.join(\".current\");

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
";

/// App crate Cargo.toml template.
pub const APP_CARGO: &str = "[package]
name = \"{name}\"
version = \"0.1.0\"
edition = \"2021\"

[lib]
name = \"{name}\"
crate-type = [\"cdylib\", \"rlib\"]
path = \"src/lib.rs\"

[dependencies]
protocol = {{ path = \"../protocol\" }}
ratatui = {{ workspace = true }}
crossterm = {{ workspace = true }}
serde = {{ workspace = true }}
serde_json = {{ workspace = true }}

[build-dependencies]
rune = {{ path = \"../../..\" }}
";

/// App lib.rs template - This gets replaced by the compiler.
/// The generated lib.rs in target/rune-cache/ includes the mod generated; declaration.
pub const APP_LIB: &str = "//! # App Library
//!
//! Hot-reloadable application logic.
//! Generated by rune - DO NOT EDIT

mod native;

use protocol::{App, AppState};

/// Create a new app instance.
#[no_mangle]
pub extern \"C\" fn create_app() -> *mut dyn App {
    Box::into_raw(Box::new(AppImpl::default()))
}

#[derive(Default)]
struct AppImpl;

impl App for AppImpl {
    fn update(&mut self, state: &mut AppState) {
        // Generated code will be inserted here by rune compiler
    }

    fn render(&self, f: &mut ratatui::Frame<'_>, state: &AppState) {
        // Generated code will be inserted here by rune compiler
    }

    fn handle_key(&mut self, _key: crossterm::event::KeyEvent, _state: &mut AppState) {}
}
";

/// Native module template.
pub const NATIVE_MOD: &str = "pub mod fast_math;
";

/// Fast math template.
pub const FAST_MATH: &str = "//! Fast math utilities written in Rust.

/// Fast square root approximation.
pub fn fast_sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// Fast sine approximation using polynomial.
pub fn fast_sin(x: f64) -> f64 {
    x.sin()
}
";

/// Main.r.ts template.
pub const MAIN_RS: &str = "//! main.r.ts - Main entry point for the app logic

import { AppState, Task, Filter } from \"./state.r.ts\";

/// Update application state.
export function update(state: AppState): void {
    if (state.selected >= state.tasks.length) {
        state.selected = Math.max(0, state.tasks.length - 1);
    }
}
";

/// State.r.ts template.
pub const STATE_RS: &str = "//! state.r.ts - Application state types

export type Task = {
    id: number;
    title: string;
    done: boolean;
};

export enum Filter {
    All = \"all\",
    Active = \"active\",
    Completed = \"completed\",
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
";

/// Root.r.tsx template.
pub const ROOT_RSX: &str = "//! root.r.tsx - Main view component

import { AppState, Filter } from \"../state.r.ts\";

export function render(state: AppState): Paragraph {
    const title = `TODOX - ${state.tasks.length} tasks`;
    let content = title + \"\\n\";
    content += \"-\".repeat(40) + \"\\n\";
    for (let i = 0; i < state.tasks.length; i++) {
        const task = state.tasks[i];
        const marker = task.done ? \"[x]\" : \"[ ]\";
        const prefix = i === state.selected ? \"> \" : \"  \";
        content += `${prefix}${marker} ${task.title}\\n`;
    }
    content += \"\\nPress q to quit, j/k to navigate, x to toggle\";
    return Paragraph::new(content);
}
";

/// Task list.r.tsx template.
pub const TASK_LIST_RSX: &str = "//! task_list.r.tsx - Task list component

import { Task, Filter } from \"../state.r.ts\";

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
