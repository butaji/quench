//! # Emitter Module
//!
//! Transpiles TypeScript to Rust.

mod core;
mod expr;
mod stmt;
mod module;
mod utils;

pub use core::{RustEmitter, EmitOptions};
pub use expr::ExprTranspiler;

pub use utils::to_snake_case;
