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

pub use ast_walker::AstWalker;
pub use calls::emit_call;
pub use code_emitter::CodeEmitter;
pub use core::{EmitOptions, RustEmitter};
pub use expressions::emit_expr;
pub use infer::infer_type;
pub use literals::emit_lit;
pub use members::{emit_member, emit_object};
pub use module::emit_module;
pub use statements::{emit_body_stmt, emit_single_stmt};
pub use switch_match::emit_switch;
pub use type_resolver::TypeResolver;
pub use types::{is_enum_type, to_rust_name, EnumDefinition, EnumVariant, RustType, StructFields};
pub use utils::{escape_rust_keyword, infer_struct_from_object, to_pascal_case, to_snake_case};
pub use variables::emit_var_decl;
