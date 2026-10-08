#![expect(clippy::missing_errors_doc)]

mod ast_impl;
mod diagnostics;
mod options;
mod parser;
mod surrogate_pair;

const STACK_EXHAUSTION_ERROR_CODE: &str = "quench-stack-exhausted";

/// Resource faults remain distinct from invalid RegExp syntax.
pub fn is_stack_exhaustion(error: &oxc_diagnostics::OxcDiagnostic) -> bool {
    error.code.number.as_deref() == Some(STACK_EXHAUSTION_ERROR_CODE)
}

mod generated {
    #[cfg(debug_assertions)]
    mod assert_layouts;
    mod derive_clone_in;
    mod derive_content_eq;
}

pub mod ast;
pub use crate::{
    ast_impl::support::{
        RegexUnsupportedFlags, RegexUnsupportedPatterns, has_unsupported_regular_expression_flags,
        has_unsupported_regular_expression_pattern,
    },
    ast_impl::visit,
    options::Options,
    parser::{ConstructorParser, LiteralParser},
};
