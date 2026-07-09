//! Lower SWC AST to runtime AST
//!
//! Converts swc_ecma_ast nodes to our runtime AST representation.

pub mod control_flow;
pub mod helpers;
pub mod jsx;
pub mod literals;
pub mod pattern;
pub mod stmt;
pub mod expr;

pub use helpers::{LowerError, atom_to_string, wtf8_atom_to_string};
pub use stmt::{lower_module, lower_script, lower_stmt};
pub use expr::lower_expr;

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    use crate::hir::HirFunction;

    #[test]
    fn test_lower_simple() {
        // Just verify it compiles - test that lower_hir module is accessible
        fn _check_hir_function(_: Option<HirFunction>) {}
        _check_hir_function(None);
    }
}
