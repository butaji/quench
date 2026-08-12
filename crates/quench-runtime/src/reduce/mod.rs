//! OXC-to-residual reduction entry point.
mod module_metadata;
pub(crate) mod reduce_assignments;
mod reduce_eval;
pub mod reduce_expressions;
mod reduce_module;
mod reduce_scripts;
pub mod reduce_statements;
mod tagged_template;

// Re-export commonly used functions at the crate::reduce level
pub use module_metadata::ModuleMetadata;
pub use reduce_assignments::reduce_assignment;
pub use reduce_expressions::{
    reduce_atom, reduce_call, reduce_declaration, reduce_expression, reduce_expression_statement,
    reduce_if_statement, reduce_unary,
};
pub use reduce_scripts::{reduce_module_sequence, reduce_script_sources, ScriptSource};
pub use reduce_statements::inspect_module_source;
pub use reduce_statements::{
    reduce_eval_source, reduce_expression_statements_with_locals, reduce_function_declaration,
    reduce_module_source, reduce_source, reduce_source_with_type, reduce_statement,
    reduce_statements_no_tail, reduce_statements_with_locals, ResidualProgram,
};
