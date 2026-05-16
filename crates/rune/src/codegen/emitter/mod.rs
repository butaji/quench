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
#[allow(unused_imports)]
pub use expr::ExprTranspiler;
#[allow(unused_imports)]
pub use utils::to_snake_case;
#[allow(unused_imports)]
pub use ast_walker::AstWalker;
