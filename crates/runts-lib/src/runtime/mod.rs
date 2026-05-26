//! Runtime library for runts
//!
//! This module provides the runtime support for compiled runts applications.
//! It includes:
//! - Virtual DOM (VNode)
//! - Hooks implementation
//! - Islands architecture
//! - Server utilities

pub mod prelude;

pub mod vdom;
pub mod component;
pub mod islands;
pub mod hooks;
pub mod signals;
pub mod server;

/// Compile-time constant for browser detection
/// This is replaced with `true` or `false` at compile time
pub const IS_BROWSER: bool = false;
