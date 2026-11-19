//! The read-only checks one whole-tree walk of a parsed program serves: the nesting budget
//! (`stack`), TypeScript decorators, a `return` outside any function, and the call assignment
//! targets Annex B rejects. Each pass keeps the first finding in visit order, which is the
//! order the compiler reports them in; a caller reads each finding in its own order.

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;

use super::annex_b_targets::call_target;

/// The passes one walk serves. A pass whose flag is off records nothing, and a walk runs only
/// when some pass is on.
#[derive(Clone, Copy, Default)]
pub(super) struct Passes {
    /// The nesting budget (`stack::validate`).
    pub(super) depth: bool,
    pub(super) decorators: bool,
    pub(super) eval_return: bool,
    pub(super) targets: bool,
}

/// What the walk found. Each field is set only by a pass that was on.
#[derive(Default)]
pub(super) struct Findings {
    /// The nesting budget ran out before the tree was walked.
    pub(super) stack_exhausted: bool,
    /// The first TypeScript decorator, with the message that rejects it.
    pub(super) decorator: Option<(Span, &'static str)>,
    /// The first `return` outside every function and arrow.
    pub(super) eval_return: Option<Span>,
    /// The first call assignment target Annex B rejects.
    pub(super) invalid_target: Option<Span>,
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

/// Walks `program` once for the passes asked. The depth check runs against the budget left
/// where the walk starts, and is skipped for a tree short enough that the budget admits every
/// level (`StackBudget::admits_tree`); the other passes still walk such a tree.
pub(super) fn walk(program: &Program<'_>, passes: Passes) -> Findings {
    let budget = crate::stack::StackBudget::here();
    let budget = (passes.depth && !budget.admits_tree(depth_bound(program))).then_some(budget);
    let needed = budget.is_some() || passes.decorators || passes.eval_return || passes.targets;
    if !needed {
        return Findings::default();
    }
    let mut visitor = Walk { passes, budget, depth: 0, exhausted: false, callables: 0, findings: Findings::default() };
    visitor.visit_program(program);
    visitor.findings.stack_exhausted = visitor.exhausted;
    visitor.findings
}

struct Walk {
    passes: Passes,
    /// `None` where the depth check is skipped.
    budget: Option<crate::stack::StackBudget>,
    depth: usize,
    exhausted: bool,
    /// Functions and arrows enclosing the node being visited.
    callables: usize,
    findings: Findings,
}

impl Walk {
    /// Visits one level deeper, unless the budget is spent. Once spent, nothing below is walked:
    /// the caller reports exhaustion and no other finding is read.
    fn descend(&mut self, visit: impl FnOnce(&mut Self)) {
        if self.exhausted {
            return;
        }
        let depth = self.depth;
        if self.budget.as_ref().is_some_and(|budget| !budget.admits(depth)) {
            self.exhausted = true;
            return;
        }
        self.depth += 1;
        visit(self);
        self.depth -= 1;
    }

    fn decorated(&mut self, decorators: &[Decorator<'_>], span: Span) {
        if self.passes.decorators && !decorators.is_empty() {
            self.findings.decorator.get_or_insert((span, "TypeScript decorators need a runtime meaning"));
        }
    }
}

impl<'a> Visit<'a> for Walk {
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

    fn visit_formal_parameter(&mut self, parameter: &FormalParameter<'a>) {
        self.decorated(&parameter.decorators, parameter.span);
        walk::walk_formal_parameter(self, parameter);
    }

    fn visit_class(&mut self, class: &Class<'a>) {
        self.decorated(&class.decorators, class.span);
        walk::walk_class(self, class);
    }

    fn visit_property_definition(&mut self, property: &PropertyDefinition<'a>) {
        self.decorated(&property.decorators, property.span);
        walk::walk_property_definition(self, property);
    }

    fn visit_accessor_property(&mut self, accessor: &AccessorProperty<'a>) {
        self.decorated(&accessor.decorators, accessor.span);
        walk::walk_accessor_property(self, accessor);
    }

    fn visit_method_definition(&mut self, method: &MethodDefinition<'a>) {
        self.decorated(&method.decorators, method.span);
        walk::walk_method_definition(self, method);
    }

    fn visit_return_statement(&mut self, statement: &ReturnStatement<'a>) {
        if self.passes.eval_return && self.callables == 0 {
            self.findings.eval_return.get_or_insert(statement.span);
        }
        walk::walk_return_statement(self, statement);
    }

    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        self.callables += 1;
        walk::walk_function(self, function, flags);
        self.callables -= 1;
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        self.callables += 1;
        walk::walk_arrow_function_expression(self, arrow);
        self.callables -= 1;
    }

    fn visit_assignment_expression(&mut self, assignment: &AssignmentExpression<'a>) {
        if self.passes.targets
            && assignment.operator.is_logical()
            && assignment.left.as_simple_assignment_target().and_then(call_target).is_some()
        {
            self.findings.invalid_target.get_or_insert_with(|| assignment.left.span());
        }
        walk::walk_assignment_expression(self, assignment);
    }

    fn visit_assignment_target_maybe_default(&mut self, target: &AssignmentTargetMaybeDefault<'a>) {
        if self.passes.targets {
            let binding = match target {
                AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(default) => Some(&default.binding),
                _ => target.as_assignment_target(),
            };
            if let Some(call) = binding
                .and_then(|binding| binding.as_simple_assignment_target())
                .and_then(call_target)
            {
                self.findings.invalid_target.get_or_insert_with(|| call.span());
            }
        }
        walk::walk_assignment_target_maybe_default(self, target);
    }

    fn visit_assignment_target_rest(&mut self, rest: &AssignmentTargetRest<'a>) {
        if self.passes.targets
            && let Some(call) = rest.target.as_simple_assignment_target().and_then(call_target)
        {
            self.findings.invalid_target.get_or_insert_with(|| call.span());
        }
        walk::walk_assignment_target_rest(self, rest);
    }
}
