//! Ratatui TUI framework plugin for runts.

mod codegen;
mod plugin;

pub use codegen::{tui_main, widget_block, widget_layout, widget_text};
pub use plugin::RatatuiPlugin;

#[cfg(test)]
mod plugin_test;
