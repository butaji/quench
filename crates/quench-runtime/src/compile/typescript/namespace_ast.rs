//! Namespace declarations as JavaScript nodes, built directly with the AST builder. The
//! statements are those of tsc's emit: the name's declaration when it is the first
//! (`var N;`, or `export var N;` from a module) and the object it fills (`N || (N = {});`,
//! or `N = P.N || (P.N = {});` inside a parent `P`); the body as a block in which each
//! export is published on that object after its declaration (`N.x = x;`); and a block per
//! further name of a dotted `namespace A.B.C`. Every node carries the namespace's span, as
//! the reparsed text did.

use oxc_allocator::{Allocator, Vec as ArenaVec};
use oxc_ast::{ast::*, builder::AstBuilder};
use oxc_span::Span;

use super::admit::Export;

/// The statements that open namespace `name` in its list: its declaration when `first`, then
/// the object it fills. `kind`: the binding keyword of the enclosing list.
pub(super) fn head<'a>(allocator: &'a Allocator, span: Span, name: &str, first: bool, how: Export<'_>, kind: VariableDeclarationKind) -> Vec<Statement<'a>> {
    let emit = Emit::new(allocator, span);
    let mut statements = Vec::with_capacity(2);
    if first {
        statements.push(match how {
            Export::Module => emit.export(emit.var_declaration(VariableDeclarationKind::Var, name, None)),
            _ => emit.declare(kind, name, None),
        });
    }
    statements.push(emit.statement(emit.object(name, how)));
    statements
}

/// The publication of namespace `namespace`'s exports after their declarations: `N.x = x;` per name.
pub(super) fn publish<'a>(allocator: &'a Allocator, span: Span, namespace: &str, names: &[String]) -> Vec<Statement<'a>> {
    let emit = Emit::new(allocator, span);
    names.iter().map(|name| {
        let target = emit.static_target(emit.identifier(namespace), name);
        emit.statement(emit.assign(target, emit.identifier(name)))
    }).collect()
}

/// The block a namespace body runs in, nested in one block per further name of `names`:
/// `{ let B = A.B || (A.B = {}); { … } }` for `namespace A.B`.
pub(super) fn body<'a>(allocator: &'a Allocator, span: Span, names: &[String], statements: ArenaVec<'a, Statement<'a>>) -> Statement<'a> {
    let emit = Emit::new(allocator, span);
    let mut inner = emit.block(statements);
    for pair in names.windows(2).rev() {
        let object = emit.declare(VariableDeclarationKind::Let, &pair[1], Some(emit.member_object(&pair[0], &pair[1])));
        let mut statements = ArenaVec::with_capacity_in(2, &emit.allocator);
        statements.push(object);
        statements.push(inner);
        inner = emit.block(statements);
    }
    inner
}

struct Emit<'a> {
    allocator: &'a Allocator,
    ast: AstBuilder<'a>,
    span: Span,
}

impl<'a> Emit<'a> {
    fn new(allocator: &'a Allocator, span: Span) -> Self {
        Self { allocator, ast: AstBuilder::new(allocator), span }
    }

    fn text(&self, text: &str) -> &'a str {
        self.allocator.alloc_str(text)
    }

    fn one<T>(&self, item: T) -> ArenaVec<'a, T> {
        let mut items = ArenaVec::with_capacity_in(1, &self.allocator);
        items.push(item);
        items
    }

    fn identifier(&self, name: &str) -> Expression<'a> {
        Expression::new_identifier(self.span, self.text(name), &self.ast)
    }

    fn identifier_target(&self, name: &str) -> AssignmentTarget<'a> {
        AssignmentTarget::new_assignment_target_identifier(self.span, self.text(name), &self.ast)
    }

    fn static_member(&self, object: Expression<'a>, property: &str) -> Expression<'a> {
        let property = IdentifierName::new(self.span, self.text(property), &self.ast);
        Expression::new_static_member_expression(self.span, object, property, false, &self.ast)
    }

    fn static_target(&self, object: Expression<'a>, property: &str) -> AssignmentTarget<'a> {
        let property = IdentifierName::new(self.span, self.text(property), &self.ast);
        AssignmentTarget::new_static_member_expression(self.span, object, property, false, &self.ast)
    }

    fn empty_object(&self) -> Expression<'a> {
        Expression::new_object_expression(self.span, ArenaVec::<ObjectPropertyKind<'a>>::new_in(&self.allocator), &self.ast)
    }

    fn assign(&self, left: AssignmentTarget<'a>, right: Expression<'a>) -> Expression<'a> {
        Expression::new_assignment_expression(self.span, AssignmentOperator::Assign, left, right, &self.ast)
    }

    fn logical_or(&self, left: Expression<'a>, right: Expression<'a>) -> Expression<'a> {
        Expression::new_logical_expression(self.span, left, LogicalOperator::Or, right, &self.ast)
    }

    fn parenthesized(&self, expression: Expression<'a>) -> Expression<'a> {
        Expression::new_parenthesized_expression(self.span, expression, &self.ast)
    }

    fn statement(&self, expression: Expression<'a>) -> Statement<'a> {
        Statement::new_expression_statement(self.span, expression, &self.ast)
    }

    fn block(&self, statements: ArenaVec<'a, Statement<'a>>) -> Statement<'a> {
        Statement::new_block_statement(self.span, statements, &self.ast)
    }

    fn declarator(&self, name: &str, init: Option<Expression<'a>>) -> VariableDeclarator<'a> {
        let id = BindingPattern::new_binding_identifier(self.span, self.text(name), &self.ast);
        VariableDeclarator::new(self.span, id, None, init, false, &self.ast)
    }

    fn var_declaration(&self, kind: VariableDeclarationKind, name: &str, init: Option<Expression<'a>>) -> Declaration<'a> {
        Declaration::new_variable_declaration(self.span, kind, self.one(self.declarator(name, init)), false, &self.ast)
    }

    fn declare(&self, kind: VariableDeclarationKind, name: &str, init: Option<Expression<'a>>) -> Statement<'a> {
        Statement::new_variable_declaration(self.span, kind, self.one(self.declarator(name, init)), false, &self.ast)
    }

    fn export(&self, declaration: Declaration<'a>) -> Statement<'a> {
        Statement::new_export_declaration(self.span, declaration, &self.ast)
    }

    /// `P.N || (P.N = {})`: the object `N` of parent `P`, created when absent.
    fn member_object(&self, parent: &str, name: &str) -> Expression<'a> {
        let created = self.parenthesized(self.assign(self.static_target(self.identifier(parent), name), self.empty_object()));
        self.logical_or(self.static_member(self.identifier(parent), name), created)
    }

    /// The object a declaration of `name` fills: `N || (N = {})`, or `N = P.N || (P.N = {})`
    /// for a name of namespace `P`.
    fn object(&self, name: &str, how: Export<'_>) -> Expression<'a> {
        match how {
            Export::Namespace(parent) => self.assign(self.identifier_target(name), self.member_object(parent, name)),
            _ => {
                let created = self.parenthesized(self.assign(self.identifier_target(name), self.empty_object()));
                self.logical_or(self.identifier(name), created)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use oxc_allocator::Allocator;
    use oxc_ast_visit::VisitMut;
    use oxc_parser::Parser;
    use oxc_span::{SourceType, Span};

    use super::super::admit::admit;
    use crate::bytecode::DeclaredTypes;

    /// Erases every span, so that trees compare by their shape alone.
    struct Unspan;

    impl<'a> VisitMut<'a> for Unspan {
        fn visit_span(&mut self, span: &mut Span) {
            *span = Span::empty(0);
        }
    }

    /// Lowering `typed` admits to the statements `javascript` parses to, span for span.
    fn assert_lowers_to(typed: &str, javascript: &str) {
        let allocator = Allocator::default();
        let mut lowered = Parser::new(&allocator, typed, SourceType::ts()).parse().program;
        admit(&allocator, &mut lowered, &mut DeclaredTypes::default());
        let mut expected = Parser::new(&allocator, javascript, SourceType::mjs()).parse().program;
        let mut unspan = Unspan;
        unspan.visit_program(&mut lowered);
        unspan.visit_program(&mut expected);
        assert_eq!(format!("{:?}", lowered.body), format!("{:?}", expected.body));
    }

    #[test]
    fn namespaces_lower_to_the_statements_tsc_emits() {
        assert_lowers_to(
            "namespace A.B { export const k = 1; export function f() { return k; } const hidden = 2; }",
            "var A; A || (A = {}); { let B = A.B || (A.B = {}); { const k = 1; B.k = k; function f() { return k; } B.f = f; const hidden = 2; } }",
        );
        assert_lowers_to(
            "namespace O { export namespace I { export const x = 1; } }",
            "var O; O || (O = {}); { let I; I = O.I || (O.I = {}); { const x = 1; I.x = x; } }",
        );
    }

    #[test]
    fn exported_namespaces_publish_from_the_module_binding() {
        assert_lowers_to(
            "export namespace M { export let z = 0; }",
            "export var M; M || (M = {}); { let z = 0; M.z = z; }",
        );
    }
}
