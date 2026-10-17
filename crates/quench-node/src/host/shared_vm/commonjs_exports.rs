//! Node's static CJS export recognizers operate on syntax, never evaluated values.
use oxc_ast::ast::{
    AssignmentExpression, BindingPattern, CallExpression, Expression, ObjectExpression,
    ObjectPropertyKind, PropertyKey, PropertyKind, SimpleAssignmentTarget, Statement,
    VariableDeclarator,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::GetSpan;
use std::{collections::BTreeSet, path::Path};

pub(super) fn names(path: &Path, source: &str) -> Vec<String> {
    let resolver = super::commonjs::commonjs_resolver();
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
        required: std::collections::BTreeMap::new(),
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
    /// `var _x = require("x")` bindings, for Babel's `Object.keys(_x).forEach` re-export.
    required: std::collections::BTreeMap<String, String>,
}
/// `callee` names `object.property` or, with no object, the bare `property`.
fn callee_is(callee: &Expression<'_>, object: Option<&str>, property: &str) -> bool {
    match (callee, object) {
        (Expression::StaticMemberExpression(member), Some(object)) => {
            identifier(&member.object, object) && member.property.name == property
        }
        (Expression::StaticMemberExpression(member), None) => member.property.name == property,
        (Expression::Identifier(_), None) => identifier(callee, property),
        _ => false,
    }
}
/// The descriptor shapes cjs-module-lexer accepts: a `value`, or an enumerable getter.
fn exporting_descriptor(object: &ObjectExpression<'_>) -> bool {
    let value = |name: &str| {
        object.properties.iter().find_map(|property| match property {
            ObjectPropertyKind::ObjectProperty(property) if property.key.static_name().as_deref() == Some(name) => {
                Some(&property.value)
            }
            _ => None,
        })
    };
    value("value").is_some()
        || value("get").is_some() && matches!(value("enumerable"), Some(Expression::BooleanLiteral(flag)) if flag.value)
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
    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        if let (BindingPattern::BindingIdentifier(binding), Some(init)) = (&declarator.id, &declarator.init) {
            let specifier = require_specifier(init).or_else(|| match init {
                // `_interopRequireWildcard(require("x"))` and similar helpers.
                Expression::CallExpression(call) if call.arguments.len() == 1 => {
                    require_specifier(call.arguments[0].as_expression()?)
                }
                _ => None,
            });
            if let Some(specifier) = specifier {
                self.required.insert(binding.name.to_string(), specifier.to_owned());
            }
        }
        walk::walk_variable_declarator(self, declarator);
    }
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        let argument = |index: usize| call.arguments.get(index).and_then(|argument| argument.as_expression());
        if callee_is(&call.callee, Some("Object"), "defineProperty") {
            // `Object.defineProperty(exports, "name", { enumerable: true, get() {...} })`.
            if let (Some(target), Some(Expression::StringLiteral(name)), Some(Expression::ObjectExpression(descriptor))) =
                (argument(0), argument(1), argument(2))
            {
                if exported_object(target) && exporting_descriptor(descriptor) {
                    self.names.insert(name.value.to_string());
                }
            }
        } else if callee_is(&call.callee, None, "__exportStar") {
            // TypeScript's `__exportStar(require("x"), exports)`, also via `tslib.`.
            if let (Some(specifier), Some(target)) = (argument(0).and_then(require_specifier), argument(1)) {
                if exported_object(target) {
                    self.reexports.insert(specifier.to_owned());
                }
            }
        } else if callee_is(&call.callee, None, "__export") && call.arguments.len() == 1 {
            // Older TypeScript's `__export(require("x"))`.
            if let Some(specifier) = argument(0).and_then(require_specifier) {
                self.reexports.insert(specifier.to_owned());
            }
        } else if callee_is(&call.callee, None, "forEach") {
            // Babel's `Object.keys(_x).forEach(function (key) { ... exports[key] = _x[key] })`.
            if let Expression::StaticMemberExpression(member) = &call.callee {
                if let Expression::CallExpression(keys) = &member.object {
                    if callee_is(&keys.callee, Some("Object"), "keys") {
                        if let Some(Expression::Identifier(module)) = keys.arguments.first().and_then(|argument| argument.as_expression()) {
                            if let Some(specifier) = self.required.get(module.name.as_str()) {
                                self.reexports.insert(specifier.clone());
                            }
                        }
                    }
                }
            }
        }
        walk::walk_call_expression(self, call);
    }
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
