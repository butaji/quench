//! Node's static CJS export recognizers operate on syntax, never evaluated values.
use oxc_ast::ast::{
    AssignmentExpression, Expression, ObjectPropertyKind, PropertyKey, PropertyKind,
    SimpleAssignmentTarget, Statement,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::GetSpan;
use std::{collections::BTreeSet, path::Path};

pub(super) fn names(path: &Path, source: &str) -> Vec<String> {
    let resolver = super::commonjs::commonjs_resolver(true);
    let mut names = BTreeSet::new();
    collect(path, source, &resolver, &mut BTreeSet::new(), &mut names);
    names.remove("default");
    names.remove("module.exports");
    names.into_iter().collect()
}

fn collect(
    path: &Path,
    source: &str,
    resolver: &oxc_resolver::Resolver,
    visited: &mut BTreeSet<std::path::PathBuf>,
    names: &mut BTreeSet<String>,
) {
    let Ok(_guard) = quench_stack::StackGuard::enter() else {
        return;
    };
    if !visited.insert(path.to_path_buf()) {
        return;
    }
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::cjs()).parse();
    let mut exports = Exports {
        source,
        names: BTreeSet::new(),
        reexports: BTreeSet::new(),
    };
    exports.visit_program(&parsed.program);
    names.extend(exports.names);
    for specifier in exports.reexports {
        let Some(base) = path.parent() else {
            continue;
        };
        let Ok(resolution) = resolver.resolve(base, &specifier) else {
            continue;
        };
        let resolved = resolution.full_path();
        let resolved = std::fs::canonicalize(&resolved).unwrap_or(resolved);
        if super::source_kind(&resolved).ok() != Some(quench_runtime::SourceKind::Script) {
            continue;
        }
        if let Ok(source) = std::fs::read_to_string(&resolved) {
            collect(&resolved, &source, resolver, visited, names);
        }
    }
}
struct Exports<'s> {
    source: &'s str,
    names: BTreeSet<String>,
    reexports: BTreeSet<String>,
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
fn require_specifier<'e>(expression: &'e Expression<'_>) -> Option<&'e str> {
    let Expression::CallExpression(call) = expression else {
        return None;
    };
    if !identifier(&call.callee, "require") || call.arguments.len() != 1 {
        return None;
    }
    match call.arguments[0].as_expression()? {
        Expression::StringLiteral(specifier) => Some(specifier.value.as_str()),
        _ => None,
    }
}
// Node recognizes an identifier prefix, even when the complete expression is
// more complex. OXC remains the authority for the surrounding syntax and spans.
fn identifier_prefix(source: &str) -> Option<&str> {
    let mut chars = source.char_indices();
    let (_, first) = chars.next()?;
    if !oxc_syntax::identifier::is_identifier_start(first) {
        return None;
    }
    let end = chars
        .find(|(_, ch)| !oxc_syntax::identifier::is_identifier_part(*ch))
        .map_or(source.len(), |(offset, _)| offset);
    Some(&source[..end])
}
impl<'a> Visit<'a> for Exports<'_> {
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
                if let Some(specifier) = require_specifier(&assignment.right) {
                    self.reexports.insert(specifier.to_owned());
                }
                if let Expression::ObjectExpression(object) = &assignment.right {
                    // Names survive when a later token ends Node's lexical
                    // recognition. In particular, `re: internal.re` contributes
                    // `re`, but prevents discovery of subsequent properties.
                    for property in &object.properties {
                        let ObjectPropertyKind::ObjectProperty(property) = property else {
                            if let ObjectPropertyKind::SpreadProperty(spread) = property {
                                if let Expression::Identifier(_) = &spread.argument {
                                    let raw = spread.argument.span().source_text(self.source);
                                    if identifier_prefix(raw) == Some(raw) {
                                        continue;
                                    }
                                }
                                if let Some(specifier) = require_specifier(&spread.argument) {
                                    self.reexports.insert(specifier.to_owned());
                                    continue;
                                }
                            }
                            break;
                        };
                        if property.computed || property.kind == PropertyKind::Get {
                            break;
                        }
                        if property.method || property.kind == PropertyKind::Set {
                            if let Some(name) =
                                identifier_prefix(property.span.source_text(self.source))
                            {
                                self.names.insert(name.to_owned());
                            }
                            break;
                        }
                        let name = match &property.key {
                            PropertyKey::StaticIdentifier(name) => name.name.as_str(),
                            PropertyKey::StringLiteral(name) => name.value.as_str(),
                            _ => break,
                        };
                        let raw = property.value.span().source_text(self.source);
                        let Some(prefix) = identifier_prefix(raw) else {
                            break;
                        };
                        self.names.insert(name.to_owned());
                        if !property.shorthand
                            && (prefix.len() != raw.len()
                                || !matches!(
                                    self.source
                                        .as_bytes()
                                        .get(property.value.span().end as usize),
                                    Some(b',' | b'}')
                                ))
                        {
                            break;
                        }
                    }
                }
            }
        }
        walk::walk_assignment_expression(self, assignment);
    }
}
