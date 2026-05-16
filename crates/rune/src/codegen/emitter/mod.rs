//! # Emitter Module
//!
//! Transpiles TypeScript to Rust.

mod ast_walker;
mod code_emitter;
mod core;
mod expr;
mod module;
mod type_resolver;
mod types;
mod utils;

pub use code_emitter::CodeEmitter;
pub use core::{RustEmitter, EmitOptions};
pub use type_resolver::TypeResolver;
pub use types::{RustType, to_snake_case};
#[allow(unused_imports)]
pub use ast_walker::AstWalker;
#[allow(unused_imports)]
pub use expr::ExprTranspiler;
