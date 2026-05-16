//! Rune application wiring layer
//!
//! This file is hand-written and references generated Rune modules.
//! The build.rs copies generated code to OUT_DIR before compilation.

mod native;

// Generated Rune modules - included from OUT_DIR by build.rs
include!(concat!(env!("OUT_DIR"), "/generated/mod.rs"));

pub use protocol::{App, AppState};
