//! Protocol crate - shared interface between host and app.

use serde::{Deserialize, Serialize};

/// Application state - owned by host, shared with app dylib.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppState {
    /// Current tasks
    pub tasks: Vec<Task>,
    /// Selected task index
    pub selected: usize,
    /// Current filter
    pub filter: Filter,
    /// Exit flag
    pub should_exit: bool,
}

/// A task item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique identifier
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
    /// Update application state based on events.
    fn update(&mut self, state: &mut AppState);

    /// Render the application to terminal frame.
    fn render(&self, f: &mut ratatui::Frame<'_>, state: &AppState);

    /// Handle a key event.
    fn handle_key(&mut self, key: crossterm::event::KeyEvent, state: &mut AppState);
}
