//! Native module - exports Rust types and functions for use in .r.ts files.

pub mod fast_math;

// Re-export types from external crates for use in generated code
pub use ratatui::Frame as Frame;
pub use crossterm::event::KeyEvent as KeyEvent;
