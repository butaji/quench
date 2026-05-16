//! # Emitter Module
//!
//! Transpiles TypeScript to Rust.

mod ast_walker;
mod code_emitter;
mod core;
mod expr;
mod module;
mod statements;
mod type_resolver;
mod types;
mod utils;
mod expressions;

pub use statements::{emit_body_stmt, emit_single_stmt};

pub use code_emitter::CodeEmitter;
pub use core::{RustEmitter, EmitOptions};
pub use type_resolver::TypeResolver;
pub use types::{RustType, to_snake_case};
#[allow(unused_imports)]
pub use ast_walker::AstWalker;
#[allow(unused_imports)]
pub use expr::ExprTranspiler;
