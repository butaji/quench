//! # Protocol
//!
//! Shared protocol between host and app dylib.
//! Contains AppState and App trait.

use serde::{Deserialize, Serialize};

/// Widget type alias for convenience.
pub type Widget = Box<dyn ratatui::widgets::Widget>;

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
/// Renders directly to terminal frame for dyn-compatibility.
pub trait App {
    /// Update application state.
    fn update(&mut self, state: &mut AppState);

    /// Render the application to a terminal frame.
    fn render(&self, f: &mut ratatui::Frame<'_>, state: &AppState);

    /// Handle key events.
    fn handle_key(&mut self, key: crossterm::event::KeyEvent, state: &mut AppState);
}

/// Message types for the application.
#[derive(Debug, Clone)]
pub enum Message {
    /// Add a new task
    AddTask(String),
    /// Toggle task completion
    ToggleTask(i32),
    /// Delete a task
    DeleteTask(i32),
    /// Select a task
    SelectTask(usize),
    /// Filter tasks
    SetFilter(Filter),
    /// Quit the application
    Quit,
}
