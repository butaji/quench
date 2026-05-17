//! # Protocol
//!
//! Shared protocol between host and app dylib.

use serde::{Deserialize, Serialize};
use ratatui::Frame;

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
/// The `'static` bound is required because the app is loaded from a dylib.
pub trait App: 'static {
    /// Update application state.
    fn update(&mut self, state: &mut AppState);

    /// Render the application to a terminal frame.
    fn render(&self, f: &mut Frame<'_>, state: &AppState);

    /// Handle key events.
    fn handle_key(&mut self, key: crossterm::event::KeyEvent, state: &mut AppState);
}
