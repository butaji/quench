//! # Emitter Module
//!
//! Transpiles TypeScript to Rust.

mod core;
mod expr;
mod stmt;
mod module;
mod utils;
mod ast_walker;

pub use core::{RustEmitter, EmitOptions};
pub use expr::ExprTranspiler;
pub use utils::to_snake_case;
pub use ast_walker::AstWalker;
