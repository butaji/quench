//! Function expressions whose own `prototype` no program text can observe.
//!
//! An ordinary function expression gets a fresh `prototype` object when it is evaluated.
//! A program reads that object only through the function value. Take a function
//! expression stored only into a local binding by a plain `=` whose value goes nowhere,
//! where the binding is read only as a callee (`f(...)`) and never otherwise read, aliased
//! or rebound: it reaches no other value, since a call of it binds `this` from its
//! receiver, not the function itself. Its own body cannot name it either: it is anonymous
//! and reads no `arguments` object, whose `callee` would. The one other way to reach it is a
//! callee's legacy `caller`, which makes the `prototype` first (`Vm::function_caller_value`).
//! A direct `eval` can read any binding of the scopes around it, so their bindings make no
//! claim. A `with` object can only redirect a name the analysis classified by its syntax,
//! which hands the binding's value nowhere new.

use oxc_ast::ast::{
    AssignmentExpression, AssignmentOperator, AssignmentTarget, CallExpression, Expression,
    ExpressionStatement, Function, IdentifierReference, SequenceExpression, VariableDeclarator,
};
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{ReferenceId, Semantic, SymbolId};
use oxc_span::Span;
use oxc_syntax::scope::ScopeFlags;
use rustc_hash::{FxHashMap, FxHashSet};

/// How one identifier reference uses its binding.
#[derive(Clone, Copy)]
enum Use {
    /// The whole callee of a call.
    Callee,
    /// The target of a discarded `=` of the function expression starting there.
    StoresFunction(u32),
    /// Anything else, which may let the value go anywhere.
    Other,
}

/// What a `var`, `let` or `const` declarator binds its name to first.
enum Initial {
    Nothing,
    Function(u32),
    Other,
}

/// The source starts of the function expressions whose `prototype` is unobservable.
pub(super) fn prototype_unobservable_functions<'a>(
    semantic: &Semantic<'a>,
    program: &oxc_ast::ast::Program<'a>,
) -> FxHashSet<u32> {
    let scoping = semantic.scoping();
    // A direct `eval` reads any binding of the scopes around it.
    let evaluated: FxHashSet<_> = scoping
        .scope_descendants_from_root()
        .filter(|scope| scoping.scope_flags(*scope).contains(ScopeFlags::DirectEval))
        .flat_map(|scope| scoping.scope_ancestors(scope))
        .collect();
    let mut uses = References::default();
    uses.visit_program(program);
    let root = scoping.root_scope_id();
    let mut found = FxHashSet::default();
    for (symbol, initial) in &uses.declarators {
        let scope = scoping.symbol_scope_id(*symbol);
        if scope == root
            || evaluated.contains(&scope)
            || !scoping.symbol_redeclarations(*symbol).is_empty()
        {
            continue;
        }
        let mut stored = match initial {
            Initial::Nothing => Vec::new(),
            Initial::Function(start) => vec![*start],
            Initial::Other => continue,
        };
        let only_called = scoping
            .get_resolved_reference_ids(*symbol)
            .iter()
            .all(|reference| match uses.references.get(reference) {
                Some(Use::Callee) => true,
                Some(Use::StoresFunction(start)) => {
                    stored.push(*start);
                    true
                }
                Some(Use::Other) | None => false,
            });
        if only_called {
            found.extend(
                stored
                    .into_iter()
                    .filter(|start| uses.plain_functions.contains(start)),
            );
        }
    }
    found
}

/// One walk of the program: each identifier reference's use, each simple declarator's
/// first value, and the anonymous ordinary function expressions that read no `arguments`.
#[derive(Default)]
struct References {
    references: FxHashMap<ReferenceId, Use>,
    declarators: Vec<(SymbolId, Initial)>,
    plain_functions: FxHashSet<u32>,
    /// Every `arguments` read, whichever function's object it names.
    arguments_reads: Vec<Span>,
    /// The function expressions seen so far, checked against `arguments_reads` at the end.
    functions: Vec<Span>,
}

impl References {
    fn mark(&mut self, reference: &IdentifierReference<'_>, usage: Use) {
        if let Some(id) = reference.reference_id.get() {
            self.references.entry(id).or_insert(usage);
        }
    }

    /// A value the program discards: a `=` of a function expression to a binding there
    /// stores it and nothing else.
    fn discarded(&mut self, expression: &Expression<'_>) {
        if let Expression::AssignmentExpression(assignment) = expression.without_parentheses() {
            self.discarded_assignment(assignment);
        }
    }

    fn discarded_assignment(&mut self, assignment: &AssignmentExpression<'_>) {
        if assignment.operator == AssignmentOperator::Assign
            && let AssignmentTarget::AssignmentTargetIdentifier(target) = &assignment.left
            && let Expression::FunctionExpression(function) = assignment.right.without_parentheses()
        {
            self.mark(target, Use::StoresFunction(function.span.start));
        }
    }
}

impl<'a> Visit<'a> for References {
    fn visit_program(&mut self, it: &oxc_ast::ast::Program<'a>) {
        walk::walk_program(self, it);
        let reads = std::mem::take(&mut self.arguments_reads);
        for function in std::mem::take(&mut self.functions) {
            if !reads
                .iter()
                .any(|read| function.start <= read.start && read.end <= function.end)
            {
                self.plain_functions.insert(function.start);
            }
        }
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if it.name == "arguments" {
            self.arguments_reads.push(it.span);
        }
        self.mark(it, Use::Other);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Expression::Identifier(callee) = &it.callee {
            self.mark(callee, Use::Callee);
        }
        walk::walk_call_expression(self, it);
    }

    fn visit_expression_statement(&mut self, it: &ExpressionStatement<'a>) {
        self.discarded(&it.expression);
        walk::walk_expression_statement(self, it);
    }

    fn visit_sequence_expression(&mut self, it: &SequenceExpression<'a>) {
        if let Some((_, discarded)) = it.expressions.split_last() {
            for expression in discarded {
                self.discarded(expression);
            }
        }
        walk::walk_sequence_expression(self, it);
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let oxc_ast::ast::BindingPattern::BindingIdentifier(binding) = &it.id
            && let Some(symbol) = binding.symbol_id.get()
        {
            let initial = match it.init.as_ref().map(Expression::without_parentheses) {
                None => Initial::Nothing,
                Some(Expression::FunctionExpression(function)) => {
                    Initial::Function(function.span.start)
                }
                Some(_) => Initial::Other,
            };
            self.declarators.push((symbol, initial));
        }
        walk::walk_variable_declarator(self, it);
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        if it.is_expression() && it.id.is_none() && !it.r#async && !it.generator {
            self.functions.push(it.span);
        }
        walk::walk_function(self, it, flags);
    }
}
