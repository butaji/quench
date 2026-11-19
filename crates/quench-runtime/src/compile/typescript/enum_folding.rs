//! Which plain enums may fold their member reads to the members' constants.
//!
//! A plain enum is an ordinary object that code can observe and change, so `E.M` folds only
//! when nothing in the unit can tell the difference: the enum's binding is declared once and
//! never reassigned, its object only ever receives a static member read (never escapes as a
//! value, is never indexed or exported), no member of it is written, and no code can read it
//! before its declaration runs. `admit` replaces the reads of the enums this pass returns.

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::scope::ScopeFlags;
use rustc_hash::{FxHashMap, FxHashSet};

use super::enums::member_name;

/// The plain enums of `program` whose member reads may fold. `bindings` counts the bindings
/// of each name; a name that is bound more than once (a merge, a shadow) never folds.
pub(super) fn foldable(program: &Program<'_>, bindings: &FxHashMap<String, u32>) -> FxHashSet<String> {
    let mut uses = Uses { bindings, candidates: FxHashMap::default(), functions: Vec::new(), allowed: FxHashSet::default(), dynamic: false };
    uses.visit_program(program);
    if uses.dynamic { return FxHashSet::default(); }
    uses.candidates.into_iter()
        .filter(|(_, candidate)| !candidate.escaped && !candidate.written && (candidate.inert || !candidate.hoisted_read))
        .map(|(name, _)| name)
        .collect()
}

/// One enum whose folding is still possible.
struct Candidate {
    /// The function nesting depth of the statement list that declares it.
    depth: usize,
    /// Whether no statement before the declaration runs code, so no hoisted function can run before it.
    inert: bool,
    /// Whether a function declaration nested in the declaring list reads it.
    hoisted_read: bool,
    /// Whether its object is used other than as the receiver of a static member read.
    escaped: bool,
    /// Whether one of its members is written, deleted or updated.
    written: bool,
}

struct Uses<'b> {
    bindings: &'b FxHashMap<String, u32>,
    candidates: FxHashMap<String, Candidate>,
    /// Whether each enclosing function is a declaration (hoisted), innermost last.
    functions: Vec<bool>,
    /// Identifier positions that are the object of a static member read.
    allowed: FxHashSet<u32>,
    /// A `with` statement or a direct `eval` can reach any binding: nothing folds.
    dynamic: bool,
}

impl<'b> Uses<'b> {
    /// Candidates of a statement list: the module's top level, or a function body. A script's
    /// top level is a global object's property, reachable from outside the unit, so it is not one.
    fn scope(&mut self, statements: &[Statement<'_>]) {
        let depth = self.functions.len();
        for (index, statement) in statements.iter().enumerate() {
            let Statement::TSEnumDeclaration(enumeration) = statement else { continue };
            let name = enumeration.id.name.as_str();
            let proto = enumeration.body.members.iter().any(|member| member_name(&member.id).as_deref() == Some("__proto__"));
            if enumeration.r#const || enumeration.declare || proto || self.bindings.get(name) != Some(&1) { continue; }
            let before_inert = statements[..index].iter().all(inert);
            self.candidates.insert(name.to_owned(), Candidate { depth, inert: before_inert, hoisted_read: false, escaped: false, written: false });
        }
    }

    fn written(&mut self, name: &str) {
        if let Some(candidate) = self.candidates.get_mut(name) { candidate.written = true; }
    }
}

/// A statement that runs no code: a declaration (a function nothing can call before it runs, or a type) or nothing.
fn inert(statement: &Statement<'_>) -> bool {
    matches!(statement, Statement::FunctionDeclaration(_) | Statement::TSInterfaceDeclaration(_)
        | Statement::TSTypeAliasDeclaration(_) | Statement::EmptyStatement(_))
}

/// The expression under parentheses and type assertions, which do not change the object.
fn strip<'e, 'a>(mut expression: &'e Expression<'a>) -> &'e Expression<'a> {
    loop {
        expression = match expression {
            Expression::ParenthesizedExpression(inner) => &inner.expression,
            Expression::TSAsExpression(inner) => &inner.expression,
            Expression::TSSatisfiesExpression(inner) => &inner.expression,
            Expression::TSNonNullExpression(inner) => &inner.expression,
            Expression::TSTypeAssertion(inner) => &inner.expression,
            _ => return expression,
        };
    }
}

/// The identifier an expression names, under parentheses and assertions.
fn base_identifier<'e, 'a>(expression: &'e Expression<'a>) -> Option<&'e IdentifierReference<'a>> {
    match strip(expression) {
        Expression::Identifier(identifier) => Some(&**identifier),
        _ => None,
    }
}

/// The enum whose member a written or deleted expression names (`E.M`, `(E as any).M`).
fn member_base<'e, 'a>(expression: &'e Expression<'a>) -> Option<&'e str> {
    let member = strip(expression).as_member_expression()?;
    base_identifier(member.object()).map(|identifier| identifier.name.as_str())
}

/// The enum whose member an assignment target names.
fn target_base<'e, 'a>(target: &'e SimpleAssignmentTarget<'a>) -> Option<&'e str> {
    match target.as_member_expression() {
        Some(member) => base_identifier(member.object()).map(|identifier| identifier.name.as_str()),
        None => member_base(target.get_expression()?),
    }
}

impl<'a, 'b> Visit<'a> for Uses<'b> {
    fn visit_program(&mut self, program: &Program<'a>) {
        if program.source_type.is_module() { self.scope(&program.body); }
        walk::walk_program(self, program);
    }

    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        self.functions.push(function.r#type == FunctionType::FunctionDeclaration);
        walk::walk_function(self, function, flags);
        self.functions.pop();
    }

    fn visit_function_body(&mut self, body: &FunctionBody<'a>) {
        self.scope(&body.statements);
        walk::walk_function_body(self, body);
    }

    /// Types are erased: a name in a type position does not reach the object.
    fn visit_ts_type(&mut self, _: &TSType<'a>) {}

    fn visit_with_statement(&mut self, statement: &WithStatement<'a>) {
        self.dynamic = true;
        walk::walk_with_statement(self, statement);
    }

    fn visit_identifier_reference(&mut self, identifier: &IdentifierReference<'a>) {
        if identifier.name == "eval" { self.dynamic = true; }
        let allowed = self.allowed.contains(&identifier.span.start);
        if let Some(candidate) = self.candidates.get_mut(identifier.name.as_str()) && !allowed {
            candidate.escaped = true;
        }
    }

    fn visit_static_member_expression(&mut self, member: &StaticMemberExpression<'a>) {
        if let Some(identifier) = base_identifier(&member.object)
            && let Some(candidate) = self.candidates.get_mut(identifier.name.as_str())
        {
            self.allowed.insert(identifier.span.start);
            candidate.hoisted_read |= self.functions[candidate.depth..].iter().any(|&declaration| declaration);
        }
        walk::walk_static_member_expression(self, member);
    }

    fn visit_simple_assignment_target(&mut self, target: &SimpleAssignmentTarget<'a>) {
        if let Some(name) = target_base(target) { self.written(name); }
        walk::walk_simple_assignment_target(self, target);
    }

    fn visit_unary_expression(&mut self, unary: &UnaryExpression<'a>) {
        if unary.operator == UnaryOperator::Delete && let Some(name) = member_base(&unary.argument) {
            self.written(name);
        }
        walk::walk_unary_expression(self, unary);
    }
}
