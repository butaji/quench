//! Protect recursive projections of OXC's RegExp AST before lowering.
use oxc::regular_expression::{
    ast::{CharacterClass, Pattern, Term},
    visit::{Visit, walk},
};

#[derive(Default)]
struct Validator {
    exhausted: bool,
    node_unsupported_escape: bool,
}

impl Validator {
    fn descend(&mut self, visit: impl FnOnce(&mut Self)) {
        if self.exhausted {
            return;
        }
        let Ok(_guard) = quench_stack::StackGuard::enter() else {
            self.exhausted = true;
            return;
        };
        visit(self);
    }
}

impl<'a> Visit<'a> for Validator {
    fn visit_term(&mut self, term: &Term<'a>) {
        if matches!(
            term,
            Term::BoundaryAssertion(assertion)
                if matches!(
                    assertion.kind,
                    oxc::regular_expression::ast::BoundaryAssertionKind::StartBuffer
                        | oxc::regular_expression::ast::BoundaryAssertionKind::EndBuffer
                        | oxc::regular_expression::ast::BoundaryAssertionKind::EndBufferOptionalNewline
                )
        ) {
            self.node_unsupported_escape = true;
        }
        self.descend(|this| walk::walk_term(this, term));
    }

    fn visit_character_class(&mut self, class: &CharacterClass<'a>) {
        self.descend(|this| walk::walk_character_class(this, class));
    }
}

pub(crate) fn validate(pattern: &Pattern<'_>) -> Result<(), String> {
    let mut validator = Validator::default();
    validator.visit_pattern(pattern);
    if validator.exhausted {
        Err(quench_stack::STACK_EXHAUSTED_MESSAGE.into())
    } else if validator.node_unsupported_escape {
        Err("Invalid escape".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn regression_recursive_regexp_compilation_exhausts_and_recovers() {
        const PARSER_STRESS_DEPTH: usize = 20_000;
        std::thread::Builder::new()
            .stack_size(quench_stack::WORKER_STACK_SIZE)
            .spawn(|| {
                let depth = PARSER_STRESS_DEPTH;
                for (source, flags) in [
                    (format!("{}a{}", "(".repeat(depth), ")".repeat(depth)), ""),
                    (format!("{}a{}", "(?:".repeat(depth), ")".repeat(depth)), ""),
                    (format!("{}a{}", "(?=".repeat(depth), ")".repeat(depth)), ""),
                    (format!("{}a{}", "[".repeat(depth), "]".repeat(depth)), "v"),
                ] {
                    let error = crate::Regex::with_flags(&source, crate::Flags::from(flags))
                        .err()
                        .unwrap();
                    assert_eq!(error, quench_stack::STACK_EXHAUSTED_MESSAGE);
                    assert_eq!(
                        crate::validate_property_escapes(&source, flags).unwrap_err(),
                        quench_stack::STACK_EXHAUSTED_MESSAGE
                    );
                    let regex = crate::Regex::with_flags("a", crate::Flags::default()).unwrap();
                    assert_eq!(regex.find_from("a", 0).next().unwrap().range, 0..1);
                }
                let error = crate::Regex::with_flags("(", crate::Flags::default())
                    .err()
                    .unwrap();
                assert_ne!(error, quench_stack::STACK_EXHAUSTED_MESSAGE);
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
