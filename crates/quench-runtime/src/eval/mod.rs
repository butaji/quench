//! JavaScript evaluation module
//!
//! Contains the expression and statement evaluators for the interpreter.

pub mod call;
pub mod class;
pub mod expression;
pub mod function;
pub mod iteration;
pub mod jsx;
pub mod literal;
pub mod member;
pub mod object;
pub mod operators;
pub mod statement;
pub mod string_methods;

pub use expression::eval_expression;
pub use function::{call_js_function_with_this, call_value, call_value_with_this};
pub use iteration::{get_enumerable_keys, get_iterator};
pub use literal::{eval_property_key, get_super_value};
pub use object::{assign_to, call_getter, call_setter, eval_callee_with_this};
pub use operators::{eval_binary_op, eval_unary_op};
pub use statement::{eval_function_body, eval_statement, eval_statements};
