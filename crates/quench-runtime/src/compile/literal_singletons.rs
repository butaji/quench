//! Literal singletons: a function's `var` bindings that hold one literal from their
//! initialization on. A closure the function creates after such a declaration ran reads the
//! literal at every read (`FunctionCompiler::folded_capture`): the binding's only other value
//! is `undefined` before the declaration, which no read of a closure created later observes,
//! and every activation's binding takes the same literal. The compiler reaches a closure's
//! creation after compiling the declaration exactly when the closure is created after the
//! declaration ran, since the declaration stands at the body's statement level and hoisted
//! declarations are compiled before the body (`CaptureScope::folds`).

use oxc_ast::ast::{
    BindingPattern, CallExpression, Expression, FormalParameters, SimpleAssignmentTarget, Statement,
    UnaryExpression, VariableDeclarationKind,
};
use oxc_ast_visit::{Visit, walk};

use super::{Atom, FunctionOptions};

/// The function whose literal singletons are asked for, and what decides that it may have
/// any: a nested function (a script's vars are the global object's properties), resolving
/// names statically, with no implicit body.
pub(super) struct Owner<'o, 'a> {
    pub(super) parent: Option<u32>,
    pub(super) binds_dynamically: bool,
    pub(super) options: &'o FunctionOptions<'a>,
}

impl Owner<'_, '_> {
    /// The declarations of the function's literal singletons, by their binding's source
    /// position, sorted.
    pub(super) fn declarations(&self, atoms: &super::atoms::CompilerAtoms<'_>, params: &[Atom], body: &[Statement<'_>]) -> Vec<u32> {
        let options = self.options;
        if self.parent.is_none()
            || self.binds_dynamically
            || options.with_depth != 0
            || options.class_initializer
            || options.implicit_body.is_some()
        {
            return Vec::new();
        }
        // A parameter's binding is the var's, which `arguments` may write.
        let candidates: Vec<(&str, u32)> = candidates(body)
            .filter(|(name, _)| params.iter().all(|atom| &atoms[*atom as usize] != *name))
            .collect();
        if candidates.is_empty() {
            return Vec::new();
        }
        let mut writes = Writes { names: candidates.iter().map(|(name, _)| *name).collect(), ..Writes::default() };
        writes.visit(options.defaults, body);
        let mut found: Vec<u32> = candidates
            .iter()
            .enumerate()
            .filter(|(index, _)| !writes.dynamic && writes.bindings[*index] == 1 && writes.assignments[*index] == 0)
            .map(|(_, (_, start))| *start)
            .collect();
        found.sort_unstable();
        found
    }
}

/// The body's statement-level `var name = literal` declarations (`binding_time::expression`).
fn candidates<'b>(body: &'b [Statement<'b>]) -> impl Iterator<Item = (&'b str, u32)> {
    body.iter()
        .filter_map(|statement| match statement {
            Statement::VariableDeclaration(declaration) if declaration.kind == VariableDeclarationKind::Var => {
                Some(declaration.declarations.iter())
            }
            _ => None,
        })
        .flatten()
        .filter_map(|item| match &item.id {
            BindingPattern::BindingIdentifier(name)
                if name.name != super::early::ARGUMENTS
                    && item.init.as_ref().is_some_and(|init| super::binding_time::expression(init).is_some()) =>
            {
                Some((name.name.as_str(), name.span.start))
            }
            _ => None,
        })
}

/// What the function and every function it encloses do to the candidate `names`: how often
/// each is bound and assigned, and whether any code may bind names dynamically (a direct
/// eval, `with`, `delete` of a name).
#[derive(Default)]
struct Writes<'n> {
    names: Vec<&'n str>,
    bindings: Vec<u32>,
    assignments: Vec<u32>,
    dynamic: bool,
}

impl Writes<'_> {
    fn visit(&mut self, defaults: Option<&FormalParameters<'_>>, body: &[Statement<'_>]) {
        self.bindings = vec![0; self.names.len()];
        self.assignments = vec![0; self.names.len()];
        if let Some(params) = defaults {
            self.visit_formal_parameters(params);
        }
        for statement in body {
            self.visit_statement(statement);
        }
    }

    fn index(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|candidate| *candidate == name)
    }

    fn assign(&mut self, name: &str) {
        if let Some(index) = self.index(name) {
            self.assignments[index] += 1;
        }
    }
}

impl<'a> Visit<'a> for Writes<'_> {
    fn visit_binding_identifier(&mut self, identifier: &oxc_ast::ast::BindingIdentifier<'a>) {
        if let Some(index) = self.index(identifier.name.as_str()) {
            self.bindings[index] += 1;
        }
    }

    fn visit_simple_assignment_target(&mut self, target: &SimpleAssignmentTarget<'a>) {
        if let SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) = target {
            self.assign(identifier.name.as_str());
        }
        walk::walk_simple_assignment_target(self, target);
    }

    fn visit_assignment_target_property_identifier(
        &mut self,
        property: &oxc_ast::ast::AssignmentTargetPropertyIdentifier<'a>,
    ) {
        self.assign(property.binding.name.as_str());
        walk::walk_assignment_target_property_identifier(self, property);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        self.dynamic |= super::early::is_direct_eval_call(call);
        walk::walk_call_expression(self, call);
    }

    fn visit_with_statement(&mut self, statement: &oxc_ast::ast::WithStatement<'a>) {
        self.dynamic = true;
        walk::walk_with_statement(self, statement);
    }

    fn visit_unary_expression(&mut self, expression: &UnaryExpression<'a>) {
        self.dynamic |= expression.operator == oxc_syntax::operator::UnaryOperator::Delete
            && matches!(expression.argument.without_parentheses(), Expression::Identifier(_));
        walk::walk_unary_expression(self, expression);
    }
}
