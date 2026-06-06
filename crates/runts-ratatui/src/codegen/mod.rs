//! Ratatui codegen module

pub mod app;
pub mod ink;
pub mod ink_widget;
pub mod traversal;
pub mod expr;
pub mod vars;

pub use app::{tui_main, widget_block, widget_layout, widget_text};
pub(crate) use vars::try_codegen_jsx;
