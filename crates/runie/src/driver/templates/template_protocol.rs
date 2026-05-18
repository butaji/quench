//! # Protocol Templates
//!
//! Templates for the protocol crate.

/// Protocol lib.rs template.
pub const PROTOCOL_LIB: &str = r"//! # Protocol
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

/// Native module template.
pub const NATIVE_MOD: &str = "pub mod fast_math;\n";

/// Fast math template.
pub const FAST_MATH: &str = r"//! Fast math utilities written in Rust.

/// Fast square root approximation.
pub fn fast_sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// Fast sine approximation using polynomial.
pub fn fast_sin(x: f64) -> f64 {
    x.sin()
}
";
