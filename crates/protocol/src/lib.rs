//! # Protocol
//!
//! Shared protocol between host and app dylib.
//! The dylib mutates AppState. The host reads AppState and does I/O.

use serde::{Deserialize, Serialize};

/// Application state - owned by host, mutated by dylib.
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
    /// Input events pushed by host before each update
    #[serde(skip)]
    pub input_events: Vec<KeyEvent>,
    /// Draw commands pushed by dylib during update
    #[serde(skip)]
    pub draw_commands: Vec<DrawCmd>,
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

/// Input event passed from host to dylib.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEvent {
    /// Character key
    Char(char),
    /// Up arrow
    Up,
    /// Down arrow
    Down,
    /// Left arrow
    Left,
    /// Right arrow
    Right,
    /// Enter/Return
    Enter,
    /// Escape
    Esc,
    /// Backspace
    Backspace,
    /// Tab
    Tab,
    /// Delete
    Delete,
    /// Home
    Home,
    /// End
    End,
    /// Page up
    PageUp,
    /// Page down
    PageDown,
    /// Unmapped key
    Other,
}

/// A draw command produced by the dylib and executed by the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawCmd {
    /// Clear the screen area
    Clear,
    /// Draw text at position
    Text {
        /// Column
        x: u16,
        /// Row
        y: u16,
        /// Text content
        content: String,
    },
    /// Draw a bordered block
    Block {
        /// Column
        x: u16,
        /// Row
        y: u16,
        /// Width
        w: u16,
        /// Height
        h: u16,
        /// Optional title
        title: Option<String>,
    },
    /// Draw a list of items
    List {
        /// Column
        x: u16,
        /// Row
        y: u16,
        /// Items
        items: Vec<String>,
        /// Selected index
        selected: usize,
    },
    /// Draw a horizontal line
    Line {
        /// Column
        x: u16,
        /// Row
        y: u16,
        /// Width
        w: u16,
    },
}

/// The single export from the dylib.
///
/// # Safety
/// `state` must point to a valid `AppState` allocated by the host.
pub type UpdateFn = unsafe extern "C" fn(*mut AppState);
