//! Lower SWC AST to runtime AST
//!
//! Converts swc_ecma_ast nodes to our runtime AST representation.

pub mod control_flow;
pub mod expr;
pub mod helpers;
pub mod jsx;
pub mod literals;
pub mod opt_chain;
pub mod pattern;
pub mod stmt;

pub use expr::lower_expr;
pub use helpers::{atom_to_string, wtf8_atom_to_string, LowerError};
pub use stmt::{lower_module, lower_script, lower_stmt};

#[cfg(test)]
mod tests {
    #[test]
    fn test_lower_module_exists() {}
}
