//! Legacy compiler projections of the shared runtime stack policy.
use oxc::ast::{
    ast::{AssignmentTarget, BindingPattern, Expression, Program, Statement},
    visit::{Visit, walk},
};

const STACK_DIAGNOSTIC_PREFIX: &str = "RangeError: ";

pub(crate) fn errors() -> Vec<String> {
    vec![format!(
        "{STACK_DIAGNOSTIC_PREFIX}{}",
        quench_stack::STACK_EXHAUSTED_MESSAGE
    )]
}

pub(crate) fn is_exhaustion(errors: &[String]) -> bool {
    errors.iter().any(|error| {
        error.strip_prefix(STACK_DIAGNOSTIC_PREFIX) == Some(quench_stack::STACK_EXHAUSTED_MESSAGE)
    })
}

pub(crate) fn parser_errors_are_exhaustion(errors: &[oxc::diagnostics::OxcDiagnostic]) -> bool {
    errors
        .iter()
        .any(oxc::regular_expression::is_stack_exhaustion)
}

#[derive(Default)]
struct Validator {
    exhausted: bool,
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

pub(crate) fn validate(program: &Program<'_>) -> Result<(), Vec<String>> {
    let mut validator = Validator::default();
    validator.visit_program(program);
    if validator.exhausted {
        Err(errors())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn regression_legacy_compiler_exhaustion_is_distinct_and_recovers() {
        const COMPILER_STRESS_DEPTH: usize = 20_000;
        std::thread::Builder::new()
            .stack_size(crate::WORKER_STACK_SIZE)
            .spawn(|| {
                let depth = COMPILER_STRESS_DEPTH;
                let sources = [
                    format!("{}1{}", "(".repeat(depth), ")".repeat(depth)),
                    format!("{}0", "!".repeat(depth)),
                    format!("{}Object", "new ".repeat(depth)),
                    format!("{}0;", "if (true) ".repeat(depth)),
                    format!("let {}a{}=[];", "[".repeat(depth), "]".repeat(depth)),
                    format!("{}1;", "a=".repeat(depth)),
                    format!("{}0;{}", "{".repeat(depth), "}".repeat(depth)),
                    format!("async {}1{}", "(".repeat(depth), ")".repeat(depth)),
                    format!("{}a", "++".repeat(depth)),
                    format!("async function f() {{ {}0; }}", "await ".repeat(depth)),
                    format!("{}1", "1+".repeat(depth)),
                ];
                for (index, source) in sources.iter().enumerate() {
                    let errors = crate::reduce::reduce_source(source).err().unwrap();
                    assert!(super::is_exhaustion(&errors), "case {index}: {errors:?}");
                    assert!(crate::reduce::reduce_source("1 + 2").is_ok());
                }
                let errors = crate::reduce::reduce_source("let =").err().unwrap();
                assert!(!super::is_exhaustion(&errors));
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
