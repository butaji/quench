//! # Emitter Module
//!
//! Transpiles TypeScript to Rust.

mod ast_walker;
mod calls;
mod code_emitter;
mod core;
mod expr;
mod expressions;
mod infer;
mod literals;
mod members;
mod module;
mod statements;
mod type_resolver;
mod types;
mod utils;

pub use statements::{emit_body_stmt, emit_single_stmt};

pub use calls::emit_call;
pub use code_emitter::CodeEmitter;
pub use core::{RustEmitter, EmitOptions};
pub use expressions::emit_expr;
pub use infer::infer_type;
pub use literals::emit_lit;
pub use members::{emit_member, emit_object};
pub use type_resolver::TypeResolver;
pub use types::{RustType, to_snake_case};
#[allow(unused_imports)]
pub use ast_walker::AstWalker;
#[allow(unused_imports)]
pub use expr::ExprTranspiler;
