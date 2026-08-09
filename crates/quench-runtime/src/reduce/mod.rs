//! OXC-to-residual reduction entry point.
pub mod reduce_expressions;
pub mod reduce_statements;

// Re-export commonly used functions at the crate::reduce level
pub use reduce_expressions::{
    reduce_assignment, reduce_atom, reduce_call, reduce_declaration, reduce_expression,
    reduce_expression_statement, reduce_if_statement, reduce_unary,
};
pub use reduce_statements::{
    reduce_function_declaration, reduce_module_source, reduce_source, reduce_source_with_type,
    reduce_statement, reduce_statements_no_tail, reduce_statements_with_locals, ResidualProgram,
};
