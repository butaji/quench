#![allow(clippy::missing_errors_doc)]

mod ast_impl;
mod diagnostics;
mod options;
mod parser;
mod surrogate_pair;

mod generated {
    mod derive_clone_in;
    mod derive_content_eq;
    #[cfg(feature = "serialize")]
    mod derive_estree;
}

pub mod ast;
const STACK_EXHAUSTION_ERROR_CODE: &str = "quench-stack-exhausted";

/// Resource faults remain distinct from invalid RegExp syntax.
pub fn is_stack_exhaustion(error: &oxc_diagnostics::OxcDiagnostic) -> bool {
    error.code.number.as_deref() == Some(STACK_EXHAUSTION_ERROR_CODE)
}

pub use crate::{
    ast_impl::visit,
    options::Options,
    parser::{ConstructorParser, LiteralParser},
};

#[cfg(test)]
mod quench_stack_tests {
    use super::*;

    #[test]
    fn recursive_groups_and_unicode_sets_exhaust_distinctly_and_recover() {
        const PARSER_STRESS_DEPTH: usize = 20_000;
        std::thread::Builder::new()
            .stack_size(quench_stack::WORKER_STACK_SIZE)
            .spawn(|| {
                let allocator = oxc_allocator::Allocator::default();
                let depth = PARSER_STRESS_DEPTH;
                for (source, flags) in [
                    (format!("{}a{}", "(".repeat(depth), ")".repeat(depth)), ""),
                    (format!("{}a{}", "(?:".repeat(depth), ")".repeat(depth)), ""),
                    (format!("{}a{}", "(?=".repeat(depth), ")".repeat(depth)), ""),
                    (format!("{}a{}", "(?<=".repeat(depth), ")".repeat(depth)), ""),
                    (format!("{}a{}", "[".repeat(depth), "]".repeat(depth)), "v"),
                ] {
                    let error = LiteralParser::new(&allocator, &source, Some(flags), Options::default())
                        .parse()
                        .unwrap_err();
                    assert!(is_stack_exhaustion(&error), "{error:?}");
                    assert_eq!(error.to_string(), quench_stack::STACK_EXHAUSTED_MESSAGE);
                    let constructor_source = format!("\"{source}\"");
                    let constructor_flags = format!("\"{flags}\"");
                    let error = ConstructorParser::new(
                        &allocator,
                        &constructor_source,
                        Some(&constructor_flags),
                        Options::default(),
                    )
                    .parse()
                    .unwrap_err();
                    assert!(is_stack_exhaustion(&error), "{error:?}");
                    assert!(
                        LiteralParser::new(&allocator, "a", None, Options::default())
                            .parse()
                            .is_ok()
                    );
                }
                let error = LiteralParser::new(&allocator, "(", None, Options::default())
                    .parse()
                    .unwrap_err();
                assert!(!is_stack_exhaustion(&error));
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
