//! Enum declarations as JavaScript nodes, built directly with the AST builder. The
//! statements are those of tsc's emit: one object literal when every member folds to an
//! exact value (`var E = {"A": 0, "0": "A"};`), else the incremental form
//! (`var E; E || (E = {}); E[E["A"] = 0] = "A"; …`). Every node carries the enum's span,
//! as the reparsed text did.

use oxc_allocator::{Allocator, Vec as ArenaVec};
use oxc_ast::{ast::*, builder::AstBuilder};
use oxc_span::Span;

use super::admit::Export;
use super::enums::{self, Computed, Member, Value};
use super::namespace_ast;

/// The statements of enum `name`. `first`: the name's first declaration; `lexical`: the
/// enclosing list's generated bindings are `let`; `values`: the runtime initializers in
/// member order.
pub(super) fn statements<'a>(allocator: &'a Allocator, name: &str, members: &[(String, Member)], values: Vec<Expression<'a>>, first: bool, how: Export<'_>, lexical: bool, span: Span) -> Vec<Statement<'a>> {
    let emit = Emit::new(allocator, span);
    let kind = if lexical { VariableDeclarationKind::Let } else { VariableDeclarationKind::Var };
    if first && let Some(object) = emit.object(members) {
        return vec![emit.object_binding(name, object, how, kind)];
    }
    emit.incremental(name, members, values, first, how, kind)
}

/// The read of member `member` of enum `name` (`E["member"]`), spanning `span`.
pub(super) fn read<'a>(allocator: &'a Allocator, span: Span, name: &str, member: &str) -> Expression<'a> {
    Emit::new(allocator, span).member_read(name, member)
}

/// A folded constant's expression, spanning `span`.
pub(super) fn constant<'a>(allocator: &'a Allocator, span: Span, value: &Value) -> Expression<'a> {
    Emit::new(allocator, span).value(value)
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

    fn string(&self, value: &str) -> Expression<'a> {
        Expression::new_string_literal(self.span, self.text(value), Some(self.text(&enums::quote(value)).into()), &self.ast)
    }

    fn key(&self, value: &str) -> PropertyKey<'a> {
        PropertyKey::new_string_literal(self.span, self.text(value), Some(self.text(&enums::quote(value)).into()), &self.ast)
    }

    /// A number as the parser reads its text: a negative one is a negation of its magnitude.
    fn number(&self, number: f64) -> Expression<'a> {
        if number.is_nan() { return self.identifier("NaN"); }
        if number.is_infinite() {
            let infinity = self.identifier("Infinity");
            return if number > 0.0 { infinity } else { self.negate(infinity) };
        }
        let magnitude = number.abs();
        let text = format!("{magnitude}");
        let base = if magnitude.fract() == 0.0 { NumberBase::Decimal } else { NumberBase::Float };
        let literal = Expression::new_numeric_literal(self.span, magnitude, Some(self.text(&text).into()), base, &self.ast);
        if number.is_sign_negative() { self.negate(literal) } else { literal }
    }

    fn value(&self, value: &Value) -> Expression<'a> {
        match value {
            Value::Number(number) => self.number(*number),
            Value::String(text) => self.string(text),
        }
    }

    fn negate(&self, argument: Expression<'a>) -> Expression<'a> {
        Expression::new_unary_expression(self.span, UnaryOperator::UnaryNegation, argument, &self.ast)
    }

    fn parenthesized(&self, expression: Expression<'a>) -> Expression<'a> {
        Expression::new_parenthesized_expression(self.span, expression, &self.ast)
    }

    fn assign(&self, left: AssignmentTarget<'a>, right: Expression<'a>) -> Expression<'a> {
        Expression::new_assignment_expression(self.span, AssignmentOperator::Assign, left, right, &self.ast)
    }

    fn statement(&self, expression: Expression<'a>) -> Statement<'a> {
        Statement::new_expression_statement(self.span, expression, &self.ast)
    }

    fn member_read(&self, name: &str, member: &str) -> Expression<'a> {
        Expression::new_computed_member_expression(self.span, self.identifier(name), self.string(member), false, &self.ast)
    }

    fn member_target(&self, object: Expression<'a>, member: &str) -> AssignmentTarget<'a> {
        AssignmentTarget::new_computed_member_expression(self.span, object, self.string(member), false, &self.ast)
    }

    fn static_target(&self, object: Expression<'a>, property: &str) -> AssignmentTarget<'a> {
        let property = IdentifierName::new(self.span, self.text(property), &self.ast);
        AssignmentTarget::new_static_member_expression(self.span, object, property, false, &self.ast)
    }

    fn declarator(&self, name: &str, init: Option<Expression<'a>>) -> VariableDeclarator<'a> {
        let id = BindingPattern::new_binding_identifier(self.span, self.text(name), &self.ast);
        VariableDeclarator::new(self.span, id, None, init, false, &self.ast)
    }

    fn declaration(&self, kind: VariableDeclarationKind, name: &str, init: Option<Expression<'a>>) -> Declaration<'a> {
        Declaration::new_variable_declaration(self.span, kind, self.one(self.declarator(name, init)), false, &self.ast)
    }

    fn declare(&self, kind: VariableDeclarationKind, name: &str, init: Option<Expression<'a>>) -> Statement<'a> {
        Statement::new_variable_declaration(self.span, kind, self.one(self.declarator(name, init)), false, &self.ast)
    }

    fn export(&self, declaration: Declaration<'a>) -> Statement<'a> {
        Statement::new_export_declaration(self.span, declaration, &self.ast)
    }

    /// `var E = {…};`, `export var E = {…};` or `var E = P.E = {…};` (a namespace's member).
    fn object_binding(&self, name: &str, object: Expression<'a>, how: Export<'_>, kind: VariableDeclarationKind) -> Statement<'a> {
        match how {
            Export::None => self.declare(kind, name, Some(object)),
            Export::Module => self.export(self.declaration(VariableDeclarationKind::Var, name, Some(object))),
            Export::Namespace(parent) => {
                let target = self.static_target(self.identifier(parent), name);
                self.declare(kind, name, Some(self.assign(target, object)))
            }
        }
    }

    /// The object literal of an enum whose members all fold to exact values, with each
    /// number's reverse mapping; None when a member does not.
    fn object(&self, members: &[(String, Member)]) -> Option<Expression<'a>> {
        let mut properties = ArenaVec::<ObjectPropertyKind<'a>>::with_capacity_in(members.len() * 2, &self.allocator);
        for (member, value) in members {
            let Member::Constant(value) = value else { return None };
            let text = enums::display(value)?;
            properties.push(self.field(member, self.value(value)));
            if let Value::Number(_) = value {
                properties.push(self.field(&text, self.string(member)));
            }
        }
        Some(Expression::new_object_expression(self.span, properties, &self.ast))
    }

    fn field(&self, key: &str, value: Expression<'a>) -> ObjectPropertyKind<'a> {
        ObjectPropertyKind::new_object_property(self.span, PropertyKind::Init, self.key(key), value, false, false, false, &self.ast)
    }

    /// `var E; E || (E = {});` then one statement per member, as tsc's emit.
    fn incremental(&self, name: &str, members: &[(String, Member)], values: Vec<Expression<'a>>, first: bool, how: Export<'_>, kind: VariableDeclarationKind) -> Vec<Statement<'a>> {
        let mut statements = namespace_ast::head(self.allocator, self.span, name, first, how, kind);
        statements.reserve(members.len());
        let mut values: Vec<Option<Expression<'a>>> = values.into_iter().map(Some).collect();
        for (member, value) in members {
            let expression = self.member_assignment(name, member, value, &mut values);
            statements.push(self.statement(expression));
        }
        statements
    }

    /// The expression that defines one member: `E["A"] = "x"` for a string, and for a
    /// number `E[E["A"] = value] = "A"` (the reverse mapping too).
    fn member_assignment(&self, name: &str, member: &str, value: &Member, values: &mut [Option<Expression<'a>>]) -> Expression<'a> {
        match value {
            Member::Constant(Value::String(text)) => self.assign(self.member_target(self.identifier(name), member), self.string(text)),
            Member::Constant(Value::Number(number)) => self.reverse(name, member, self.number(*number)),
            Member::Computed(Computed::Initializer(index)) => {
                let initializer = values[*index].take().unwrap_or_else(|| unreachable!("each initializer is emitted once"));
                self.reverse(name, member, self.parenthesized(initializer))
            }
            Member::Computed(Computed::Successor(previous)) => {
                let successor = Expression::new_binary_expression(self.span, self.member_read(name, previous), BinaryOperator::Addition, self.number(1.0), &self.ast);
                self.reverse(name, member, self.parenthesized(successor))
            }
        }
    }

    /// `E[E["member"] = value] = "member"`.
    fn reverse(&self, name: &str, member: &str, value: Expression<'a>) -> Expression<'a> {
        let forward = self.assign(self.member_target(self.identifier(name), member), value);
        let target = AssignmentTarget::new_computed_member_expression(self.span, self.identifier(name), forward, false, &self.ast);
        self.assign(target, self.string(member))
    }
}
