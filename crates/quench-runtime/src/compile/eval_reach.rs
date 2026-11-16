//! Where a function's own code holds a direct eval, which may bind names in its scope.

use oxc_ast::ast::{Expression, FormalParameters, Statement};

use super::early::{body_contains_direct_eval, expression_contains_direct_eval, parameters_contain_direct_eval};

/// Whether a function's parameters or body hold a direct eval, not counting the functions
/// it makes.
pub(super) fn function_contains_direct_eval(
    defaults: Option<&FormalParameters<'_>>,
    body: &[Statement<'_>],
    expression: Option<&Expression<'_>>,
) -> bool {
    defaults.is_some_and(parameters_contain_direct_eval)
        || body_contains_direct_eval(body)
        || expression.is_some_and(expression_contains_direct_eval)
}
