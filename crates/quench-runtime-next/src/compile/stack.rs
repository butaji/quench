//! Bound AST traversal before recursive semantic and lowering passes.
use oxc_ast::ast::{AssignmentTarget, BindingPattern, Expression, Program, Statement};
use oxc_ast_visit::{Visit, walk};

#[derive(Default)]
struct Validator {
    exhausted: bool,
}

impl Validator {
    fn descend(&mut self, visit: impl FnOnce(&mut Self)) {
        if self.exhausted {
            return;
        }
        let Ok(_guard) = crate::stack::StackGuard::enter() else {
            self.exhausted = true;
            return;
        };
        visit(self);
    }
}

impl<'a> Visit<'a> for Validator {
    fn visit_expression(&mut self, expression: &Expression<'a>) {
        self.descend(|this| walk::walk_expression(this, expression));
    }

    fn visit_statement(&mut self, statement: &Statement<'a>) {
        self.descend(|this| walk::walk_statement(this, statement));
    }

    fn visit_binding_pattern(&mut self, pattern: &BindingPattern<'a>) {
        self.descend(|this| walk::walk_binding_pattern(this, pattern));
    }

    fn visit_assignment_target(&mut self, target: &AssignmentTarget<'a>) {
        self.descend(|this| walk::walk_assignment_target(this, target));
    }
}

pub(super) fn validate(program: &Program<'_>) -> Result<(), ()> {
    let mut validator = Validator::default();
    validator.visit_program(program);
    if validator.exhausted { Err(()) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use crate::Engine;

    #[test]
    fn regression_deep_ast_fails_before_semantic_analysis_and_recovers() {
        const AST_STRESS_DEPTH: usize = 20_000;
        std::thread::Builder::new()
            .stack_size(crate::WORKER_STACK_SIZE)
            .spawn(|| {
                // OXC parses a left-associative chain iteratively; later visitors recurse.
                let source = std::iter::repeat_n("1", AST_STRESS_DEPTH)
                    .collect::<Vec<_>>()
                    .join("+");
                for specialize in [Engine::specialize, Engine::specialize_unspecialized] {
                    let diagnostics = specialize(&source, "<deep-ast>").err().unwrap();
                    assert_eq!(diagnostics.len(), 1);
                    assert!(diagnostics[0].is_stack_exhausted());
                    assert!(
                        diagnostics[0]
                            .to_string()
                            .ends_with(crate::stack::STACK_EXHAUSTED_MESSAGE)
                    );
                    assert!(specialize("1 + 2", "<recovery>").is_ok());
                    let syntax = specialize("let =", "<syntax>").err().unwrap();
                    assert!(
                        syntax
                            .iter()
                            .all(|diagnostic| !diagnostic.is_stack_exhausted())
                    );
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
