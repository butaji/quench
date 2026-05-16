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
mod switch_match;
mod type_resolver;
mod types;
pub mod utils;
mod variables;

pub use statements::{emit_body_stmt, emit_single_stmt};
#[allow(unused_imports)]
pub use variables::emit_var_decl;
#[allow(unused_imports)]
pub use switch_match::emit_switch;

pub use calls::emit_call;
pub use code_emitter::CodeEmitter;
pub use core::{RustEmitter, EmitOptions};
pub use expressions::emit_expr;
pub use infer::infer_type;
pub use literals::emit_lit;
pub use members::{emit_member, emit_object};
pub use type_resolver::TypeResolver;
pub use types::{RustType, to_rust_name, is_enum_type};
pub use utils::{to_snake_case, to_pascal_case, escape_rust_keyword, infer_struct_from_object};
#[allow(unused_imports)]
pub use ast_walker::AstWalker;
#[allow(unused_imports)]
pub use expr::ExprTranspiler;
