//! Node's static CJS export recognizers operate on syntax, never evaluated values.
use oxc_ast::ast::{
    AssignmentExpression, Expression, ObjectPropertyKind, PropertyKey, PropertyKind,
    SimpleAssignmentTarget, Statement,
};
use oxc_ast_visit::{walk, Visit};
use std::collections::BTreeSet;

pub(super) fn names(source: &str) -> Vec<String> {
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::cjs()).parse();
    let mut exports = Exports {
        names: BTreeSet::new(),
    };
    exports.visit_program(&parsed.program);
    exports.names.remove("default");
    exports.names.remove("module.exports");
    exports.names.into_iter().collect()
}
struct Exports {
    names: BTreeSet<String>,
}
fn identifier(expression: &Expression<'_>, expected: &str) -> bool {
    matches!(expression, Expression::Identifier(reference) if reference.name == expected)
}
fn exported_object(expression: &Expression<'_>) -> bool {
    identifier(expression, "exports")
        || matches!(expression,
        Expression::StaticMemberExpression(member) if identifier(&member.object, "module") && member.property.name == "exports")
}
fn is_module_exports(target: &SimpleAssignmentTarget<'_>) -> bool {
    matches!(target, SimpleAssignmentTarget::StaticMemberExpression(member)
        if identifier(&member.object, "module") && member.property.name == "exports")
}
impl<'a> Visit<'a> for Exports {
    fn visit_statement(&mut self, statement: &Statement<'a>) {
        let Ok(_guard) = quench_stack::StackGuard::enter() else {
            return;
        };
        walk::walk_statement(self, statement);
    }
    fn visit_expression(&mut self, expression: &Expression<'a>) {
        let Ok(_guard) = quench_stack::StackGuard::enter() else {
            return;
        };
        walk::walk_expression(self, expression);
    }
    fn visit_assignment_expression(&mut self, assignment: &AssignmentExpression<'a>) {
        if !assignment.operator.is_assign() {
            walk::walk_assignment_expression(self, assignment);
            return;
        }
        if let Some(target) = assignment.left.as_simple_assignment_target() {
            match target {
                SimpleAssignmentTarget::StaticMemberExpression(member)
                    if exported_object(&member.object) =>
                {
                    self.names.insert(member.property.name.to_string());
                }
                SimpleAssignmentTarget::ComputedMemberExpression(member)
                    if exported_object(&member.object) =>
                {
                    if let Expression::StringLiteral(name) = &member.expression {
                        self.names.insert(name.value.to_string());
                    }
                }
                _ => {}
            }
            if is_module_exports(target) {
                if let Expression::ObjectExpression(object) = &assignment.right {
                    // Node's CJS lexer recognizes this lexical prefix. A method,
                    // accessor, spread, computed key, or non-identifier value
                    // ends recognition; already discovered names remain exports.
                    for property in &object.properties {
                        let ObjectPropertyKind::ObjectProperty(property) = property else {
                            break;
                        };
                        if property.method
                            || property.computed
                            || property.kind != PropertyKind::Init
                            || !matches!(property.value, Expression::Identifier(_))
                        {
                            break;
                        }
                        let name = match &property.key {
                            PropertyKey::StaticIdentifier(name) => name.name.as_str(),
                            PropertyKey::StringLiteral(name) => name.value.as_str(),
                            _ => break,
                        };
                        self.names.insert(name.to_owned());
                    }
                }
            }
        }
        walk::walk_assignment_expression(self, assignment);
    }
}
