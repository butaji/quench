//! App crate - hot reloadable dylib
//!
//! Contains the wiring layer that connects Rune-generated code
//! to the host binary's event loop.

mod generated;

pub use generated::*;
