//! # Emitters Module
//!
//! Code generation for different AST node types.

pub mod expr;
pub mod stmt;

pub use expr::ExprEmitter;
pub use stmt::StmtEmitter;
