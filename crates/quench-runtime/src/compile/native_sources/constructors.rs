//! Possible constructor identities through lexical bindings. These are source
//! obligations, never replacements for runtime callee identity or source guards.

use super::{DynamicFunctionKind as Kind, NativeSourceKey};
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{Scoping, SymbolId};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Kinds([bool; Kind::ALL.len()]);
impl Kinds {
    fn one(kind: Kind) -> Self {
        Self(Kind::ALL.map(|candidate| candidate == kind))
    }
    fn merge(&mut self, other: Self) {
        for (present, incoming) in self.0.iter_mut().zip(other.0) { *present |= incoming; }
    }
    fn iter(self) -> impl Iterator<Item = Kind> {
        Kind::ALL.into_iter().zip(self.0).filter_map(|(kind, present)| present.then_some(kind))
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Flow { callable: Kinds, elements: Kinds }
impl Flow {
    fn merge(&mut self, other: Self) -> bool {
        let before = *self;
        self.callable.merge(other.callable);
        self.elements.merge(other.elements);
        *self != before
    }
}

enum Term { Constructor(Kind), Binding(SymbolId), Union(Vec<Self>), Array(Vec<Self>), Iterated(Box<Self>) }
impl Term {
    fn union(terms: impl IntoIterator<Item = Self>) -> Option<Self> {
        let mut terms = terms.into_iter();
        let first = terms.next()?;
        let Some(second) = terms.next() else { return Some(first); };
        Some(Self::Union(std::iter::once(first).chain(std::iter::once(second)).chain(terms).collect()))
    }
    fn value(&self, bindings: &BTreeMap<SymbolId, Flow>) -> Flow {
        match self {
            Self::Constructor(kind) => Flow { callable: Kinds::one(*kind), ..Default::default() },
            Self::Binding(symbol) => bindings.get(symbol).copied().unwrap_or_default(),
            Self::Union(terms) => terms.iter().fold(Flow::default(), |mut value, term| { value.merge(term.value(bindings)); value }),
            Self::Array(terms) => Flow { elements: terms.iter().fold(Kinds::default(), |mut kinds, term| { kinds.merge(term.value(bindings).callable); kinds }), ..Default::default() },
            Self::Iterated(term) => Flow { callable: term.value(bindings).elements, ..Default::default() },
        }
    }
    fn dependencies(&self, symbols: &mut BTreeSet<SymbolId>) {
        match self {
            Self::Binding(symbol) => { symbols.insert(*symbol); },
            Self::Union(terms) | Self::Array(terms) => { for term in terms { term.dependencies(symbols); } },
            Self::Iterated(term) => term.dependencies(symbols),
            Self::Constructor(_) => {},
        }
    }
}

struct Constraint { binding: SymbolId, value: Term }
struct Request { callee: Term, parameters: String, body: String }
struct Discovery<'s> { scoping: &'s Scoping, constraints: Vec<Constraint>, requests: Vec<Request> }
impl Discovery<'_> {
    fn symbol(&self, name: &IdentifierReference<'_>) -> Option<SymbolId> {
        self.scoping.get_reference(name.reference_id.get()?).symbol_id()
    }
    fn expression(&self, expression: &Expression<'_>) -> Option<Term> {
        match expression.get_inner_expression() {
            Expression::Identifier(name) => {
                let alias = self.symbol(name).map(Term::Binding);
                if name.name == "Function" {
                    Term::union(std::iter::once(Term::Constructor(Kind::Ordinary)).chain(alias))
                } else { alias }
            },
            Expression::StaticMemberExpression(member) if member.property.name == "Function"
                && matches!(member.object.get_inner_expression(), Expression::Identifier(name) if name.name == "globalThis") => Some(Term::Constructor(Kind::Ordinary)),
            Expression::StaticMemberExpression(member) if member.property.name == "constructor" => {
                let kind = match member.object.get_inner_expression() {
                    Expression::FunctionExpression(function) => match (function.r#async, function.generator) {
                        (false, false) => Kind::Ordinary, (true, false) => Kind::Async,
                        (false, true) => Kind::Generator, (true, true) => Kind::AsyncGenerator,
                    },
                    Expression::ArrowFunctionExpression(function) => if function.r#async { Kind::Async } else { Kind::Ordinary },
                    _ => return None,
                };
                Some(Term::Constructor(kind))
            },
            Expression::ArrayExpression(array) => {
                let terms: Vec<_> = array.elements.iter().filter_map(|element| match element {
                    ArrayExpressionElement::SpreadElement(spread) => self.expression(&spread.argument).map(|value| Term::Iterated(Box::new(value))),
                    _ => element.as_expression().and_then(|value| self.expression(value)),
                }).collect();
                (!terms.is_empty()).then_some(Term::Array(terms))
            },
            Expression::ConditionalExpression(branch) => Term::union([&branch.consequent, &branch.alternate].into_iter().filter_map(|value| self.expression(value))),
            Expression::LogicalExpression(branch) => Term::union([&branch.left, &branch.right].into_iter().filter_map(|value| self.expression(value))),
            Expression::AssignmentExpression(assignment) if assignment.operator == AssignmentOperator::Assign => self.expression(&assignment.right),
            Expression::SequenceExpression(sequence) => self.expression(sequence.expressions.last()?),
            _ => None,
        }
    }
    fn request(&mut self, callee: &Expression<'_>, arguments: &[Argument<'_>]) {
        let Some(callee) = self.expression(callee) else { return; };
        let Some(mut strings) = arguments.iter().map(|argument| {
            match argument.as_expression()?.get_inner_expression() {
                Expression::StringLiteral(text) => Some(text.value.as_str()), _ => None,
            }
        }).collect::<Option<Vec<_>>>() else { return; };
        let body = strings.pop().unwrap_or_default().to_owned();
        self.requests.push(Request { callee, parameters: strings.join(","), body });
    }
    fn solve(&self) -> BTreeMap<SymbolId, Flow> {
        let mut dependents: BTreeMap<SymbolId, Vec<usize>> = BTreeMap::new();
        for (index, constraint) in self.constraints.iter().enumerate() {
            let mut dependencies = BTreeSet::new();
            constraint.value.dependencies(&mut dependencies);
            for dependency in dependencies { dependents.entry(dependency).or_default().push(index); }
        }
        let mut bindings = BTreeMap::new();
        let mut work: VecDeque<_> = (0..self.constraints.len()).collect();
        let mut queued = vec![true; self.constraints.len()];
        while let Some(index) = work.pop_front() {
            queued[index] = false;
            let constraint = &self.constraints[index];
            let value = constraint.value.value(&bindings);
            if value == Flow::default() { continue; }
            if bindings.entry(constraint.binding).or_default().merge(value)
                && let Some(users) = dependents.get(&constraint.binding) {
                for &user in users {
                    if !queued[user] { queued[user] = true; work.push_back(user); }
                }
            }
        }
        bindings
    }
}
impl<'a> Visit<'a> for Discovery<'_> {
    fn visit_variable_declarator(&mut self, declaration: &VariableDeclarator<'a>) {
        if let BindingPattern::BindingIdentifier(name) = &declaration.id
            && let Some(binding) = name.symbol_id.get()
            && let Some(value) = declaration.init.as_ref().and_then(|value| self.expression(value)) {
            self.constraints.push(Constraint { binding, value });
        }
        walk::walk_variable_declarator(self, declaration);
    }
    fn visit_assignment_expression(&mut self, assignment: &AssignmentExpression<'a>) {
        if assignment.operator == AssignmentOperator::Assign
            && let AssignmentTarget::AssignmentTargetIdentifier(name) = &assignment.left
            && let Some(binding) = self.symbol(name)
            && let Some(value) = self.expression(&assignment.right) {
            self.constraints.push(Constraint { binding, value });
        }
        walk::walk_assignment_expression(self, assignment);
    }
    fn visit_for_of_statement(&mut self, statement: &ForOfStatement<'a>) {
        let binding = match &statement.left {
            ForStatementLeft::VariableDeclaration(declaration) => match declaration.declarations.as_slice() {
                [declaration] => match &declaration.id { BindingPattern::BindingIdentifier(name) => name.symbol_id.get(), _ => None },
                _ => None,
            },
            ForStatementLeft::AssignmentTargetIdentifier(name) => self.symbol(name),
            _ => None,
        };
        if let Some(binding) = binding && let Some(value) = self.expression(&statement.right) {
            self.constraints.push(Constraint { binding, value: Term::Iterated(Box::new(value)) });
        }
        walk::walk_for_of_statement(self, statement);
    }
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        self.request(&call.callee, &call.arguments);
        walk::walk_call_expression(self, call);
    }
    fn visit_new_expression(&mut self, call: &NewExpression<'a>) {
        self.request(&call.callee, &call.arguments);
        walk::walk_new_expression(self, call);
    }
}

pub(super) fn sources(program: &Program<'_>, scoping: &Scoping, specialized: bool) -> Vec<NativeSourceKey> {
    let mut discovery = Discovery { scoping, constraints: Vec::new(), requests: Vec::new() };
    discovery.visit_program(program);
    let bindings = discovery.solve();
    let mut sources = BTreeSet::new();
    for request in discovery.requests {
        for kind in request.callee.value(&bindings).callable.iter() {
            sources.insert(NativeSourceKey::function_constructor(specialized, kind, &request.parameters, &request.body));
        }
    }
    sources.into_iter().collect()
}
