//! Bound AST traversal before recursive semantic and lowering passes.
use oxc_ast::ast::{AssignmentTarget, BindingPattern, Expression, Program, Statement};
use oxc_ast_visit::{Visit, walk};

/// A walk entering one transition per level, as the recursive passes after it do, against
/// the budget left where it starts (`StackBudget`).
struct Validator {
    budget: crate::stack::StackBudget,
    depth: usize,
    exhausted: bool,
}

impl Validator {
    fn descend(&mut self, visit: impl FnOnce(&mut Self)) {
        if self.exhausted {
            return;
        }
        if !self.budget.admits(self.depth) {
            self.exhausted = true;
            return;
        }
        self.depth += 1;
        visit(self);
        self.depth -= 1;
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

pub(crate) fn validate_parsed(parsed: &oxc_parser::ParserReturn<'_>) -> Result<(), crate::stack::StackExhausted> {
    if parsed.stack_exhausted {
        return Err(crate::stack::StackExhausted);
    }
    validate(&parsed.program)
}

/// Counted nesting levels one source byte can open. Each level (expression, statement, binding
/// pattern, assignment target) owns at least one byte of its span beyond its child's, except
/// the byte-free wrapper of an expression statement without its `;`, which occurs only at a
/// path's leaf.
const LEVELS_PER_SOURCE_BYTE: usize = 2;
/// The byte-free levels a path's leaf may add beyond `LEVELS_PER_SOURCE_BYTE`.
const LEAF_LEVELS: usize = 2;

/// The deepest nesting a program's text admits: `LEVELS_PER_SOURCE_BYTE` levels per byte of its
/// span, plus the leaf's.
fn depth_bound(program: &Program<'_>) -> usize {
    let bytes = program.span.end.saturating_sub(program.span.start) as usize;
    bytes * LEVELS_PER_SOURCE_BYTE + LEAF_LEVELS
}

pub(super) fn validate(program: &Program<'_>) -> Result<(), crate::stack::StackExhausted> {
    let budget = crate::stack::StackBudget::here();
    // A short text cannot nest past the budget, so the walk that would admit every level is skipped.
    if budget.admits_tree(depth_bound(program)) {
        return Ok(());
    }
    let mut validator = Validator { budget, depth: 0, exhausted: false };
    validator.visit_program(program);
    if validator.exhausted { Err(crate::stack::StackExhausted) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use crate::Engine;

    #[test]
    fn regression_generated_accessor_parser_preserves_resource_exhaustion() {
        let source = "class C { accessor x; }";
        assert!(Engine::specialize(source, "<accessor>").is_ok());
        for specialize in [Engine::specialize, Engine::specialize_unspecialized] {
            let mut guards = Vec::new();
            while let Ok(guard) = crate::stack::StackGuard::enter() {
                guards.push(guard);
            }
            // A class declaration fits in one remaining statement transition;
            // its generated getter/setter bodies require additional transitions.
            drop(guards.pop());
            let errors = specialize(source, "<accessor-budget>").err().unwrap();
            assert!(
                errors.iter().any(|error| error.is_stack_exhausted()),
                "{errors:?}"
            );
            drop(guards);
            assert!(specialize(source, "<recovery>").is_ok());
        }
    }

    #[test]
    fn regression_deep_parser_transitions_fail_as_exhaustion_and_recover() {
        const PARSER_STRESS_DEPTH: usize = 20_000;
        std::thread::Builder::new()
            .stack_size(crate::WORKER_STACK_SIZE)
            .spawn(|| {
                let depth = PARSER_STRESS_DEPTH;
                let cases = [
                    format!("{}1{}", "(".repeat(depth), ")".repeat(depth)),
                    format!("{}0", "!".repeat(depth)),
                    format!("{}Object", "new ".repeat(depth)),
                    format!("{}0;", "if (true) ".repeat(depth)),
                    format!("let {}a{} = [];", "[".repeat(depth), "]".repeat(depth)),
                    format!("{}1;", "a=".repeat(depth)),
                    format!("{}0;{}", "{".repeat(depth), "}".repeat(depth)),
                    format!("async {}1{}", "(".repeat(depth), ")".repeat(depth)),
                    format!("{}a", "++".repeat(depth)),
                    format!("async function f() {{ {}0; }}", "await ".repeat(depth)),
                ];
                for (index, source) in cases.iter().enumerate() {
                    assert!(Engine::static_module_analysis(source, "<deep-parser>").early_error.is_none());
                    for specialize in [Engine::specialize, Engine::specialize_unspecialized] {
                        let errors = specialize(source, "<deep-parser>").err().unwrap();
                        assert_eq!(errors.len(), 1, "case {index}");
                        assert!(errors[0].is_stack_exhausted(), "case {index}: {errors:?}");
                        assert!(specialize("1 + 2", "<recovery>").is_ok());
                    }
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

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
