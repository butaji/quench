//! Module forms as JavaScript nodes, built directly with the AST builder: `import x = …`,
//! `export = …`, the `export default` naming a decorated class, and a constructor's
//! parameter properties (`this.x = x;`). The expressions and names the source provides are
//! moved into the new nodes and keep their spans; the nodes built here carry the span of
//! the statement they lower, as the reparsed text did.

use oxc_allocator::{Allocator, Box as ArenaBox, Vec as ArenaVec};
use oxc_ast::{ast::*, builder::AstBuilder};
use oxc_span::Span;

use super::admit::Export;

/// `import x = <reference>`: `var x = require("m");`, `var x = A.B;` (`let` in a block),
/// `export var x = …;` from a module, or `var x = P.x = …;` for a namespace's member.
pub(super) fn import_equals<'a>(allocator: &'a Allocator, alias: TSImportEqualsDeclaration<'a>, how: Export<'_>, lexical: bool, span: Span) -> Statement<'a> {
    let ast = AstBuilder::new(allocator);
    let TSImportEqualsDeclaration { id, module_reference, .. } = alias;
    let name = allocator.alloc_str(id.name.as_str());
    let target = match module_reference {
        TSModuleReference::ExternalModuleReference(external) => require(allocator, span, external.unbox().expression),
        TSModuleReference::IdentifierReference(identifier) => Expression::Identifier(identifier),
        TSModuleReference::QualifiedName(qualified) => qualified_name(allocator, qualified.unbox()),
    };
    let init = match how {
        Export::Namespace(parent) => {
            let parent = Expression::new_identifier(span, allocator.alloc_str(parent), &ast);
            let member = AssignmentTarget::new_static_member_expression(span, parent, IdentifierName::new(span, name, &ast), false, &ast);
            Expression::new_assignment_expression(span, AssignmentOperator::Assign, member, target, &ast)
        }
        Export::None | Export::Module => target,
    };
    let binding = BindingPattern::new_binding_identifier(id.span, name, &ast);
    let declarator = VariableDeclarator::new(span, binding, None, Some(init), false, &ast);
    match how {
        Export::Module => {
            let declaration = Declaration::new_variable_declaration(span, VariableDeclarationKind::Var, one(allocator, declarator), false, &ast);
            Statement::new_export_declaration(span, declaration, &ast)
        }
        Export::None | Export::Namespace(_) => {
            let kind = if lexical { VariableDeclarationKind::Let } else { VariableDeclarationKind::Var };
            Statement::new_variable_declaration(span, kind, one(allocator, declarator), false, &ast)
        }
    }
}

/// `export = <expression>;` as `module.exports = <expression>;`.
pub(super) fn export_assignment<'a>(allocator: &'a Allocator, expression: Expression<'a>, span: Span) -> Statement<'a> {
    let ast = AstBuilder::new(allocator);
    let module = Expression::new_identifier(span, allocator.alloc_str("module"), &ast);
    let target = AssignmentTarget::new_static_member_expression(span, module, IdentifierName::new(span, allocator.alloc_str("exports"), &ast), false, &ast);
    let assignment = Expression::new_assignment_expression(span, AssignmentOperator::Assign, target, expression, &ast);
    Statement::new_expression_statement(span, assignment, &ast)
}

/// `export default <name>;`, after the statements that define a decorated class `name`.
pub(super) fn export_default_name<'a>(allocator: &'a Allocator, name: &str, span: Span) -> Statement<'a> {
    let ast = AstBuilder::new(allocator);
    let identifier = Expression::new_identifier(span, allocator.alloc_str(name), &ast);
    Statement::new_export_default_declaration(span, ExportDefaultDeclarationKind::from(identifier), &ast)
}

/// `this.x = x;` for each parameter property `x`, in order.
pub(super) fn parameter_assignments<'a>(allocator: &'a Allocator, names: &[String], span: Span) -> Vec<Statement<'a>> {
    let ast = AstBuilder::new(allocator);
    names.iter().map(|name| {
        let name = allocator.alloc_str(name);
        let this = Expression::new_this_expression(span, &ast);
        let target = AssignmentTarget::new_static_member_expression(span, this, IdentifierName::new(span, name, &ast), false, &ast);
        let value = Expression::new_identifier(span, name, &ast);
        let assignment = Expression::new_assignment_expression(span, AssignmentOperator::Assign, target, value, &ast);
        Statement::new_expression_statement(span, assignment, &ast)
    }).collect()
}

/// `require(<literal>)`, the literal moved in with its span.
fn require<'a>(allocator: &'a Allocator, span: Span, literal: StringLiteral<'a>) -> Expression<'a> {
    let ast = AstBuilder::new(allocator);
    let callee = Expression::new_identifier(span, allocator.alloc_str("require"), &ast);
    let argument = Argument::from(Expression::StringLiteral(ArenaBox::new_in(literal, &allocator)));
    Expression::new_call_expression(span, callee, None, one(allocator, argument), false, &ast)
}

/// `A.B.C` from a qualified name, its parts moved in.
fn qualified_name<'a>(allocator: &'a Allocator, name: TSQualifiedName<'a>) -> Expression<'a> {
    let ast = AstBuilder::new(allocator);
    let TSQualifiedName { span, left, right, .. } = name;
    let object = match left {
        TSTypeName::IdentifierReference(identifier) => Expression::Identifier(identifier),
        TSTypeName::QualifiedName(inner) => qualified_name(allocator, inner.unbox()),
        TSTypeName::ThisExpression(this) => Expression::ThisExpression(this),
    };
    Expression::new_static_member_expression(span, object, right, false, &ast)
}

fn one<'a, T>(allocator: &'a Allocator, item: T) -> ArenaVec<'a, T> {
    let mut items = ArenaVec::with_capacity_in(1, &allocator);
    items.push(item);
    items
}
