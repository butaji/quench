//! Possible Node loader origins. These facts archive dependencies; they never
//! replace calls or establish a runtime function's identity from a TS type.
use oxc_ast::ast::*;
use oxc_ast_visit::{walk, Visit};
use oxc_semantic::Scoping;
use oxc_syntax::{
    operator::{AssignmentOperator, LogicalOperator},
    symbol::SymbolId,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
// Every tracked origin denotes an object/function: always truthy and non-null.
enum Origin {
    ModuleNamespace,
    ModuleRecord,
    Factory,
    RequireCurrent,
    RequireMethod,
    RequireUnknown,
}
type Origins = BTreeSet<Origin>;
#[derive(Clone, Copy)]
enum Member {
    CreateRequire,
    Constructor,
    Require,
}
#[derive(Clone, Copy)]
enum Input {
    CurrentModuleBase,
    ModuleBuiltin,
    Other,
}
enum Flow {
    Binding(SymbolId, u32),
    Root(Origin, u32),
    Member(Box<Flow>, Member),
    Call(Box<Flow>, Input),
    Join(Box<Flow>, Box<Flow>),
    And(Box<Flow>, Option<Box<Flow>>),
}
struct Constraint {
    binding: SymbolId,
    value: Flow,
}

pub(super) struct LoaderBindings<'s> {
    scoping: &'s Scoping,
    commonjs: bool,
    current_file: &'s Path,
    origins: BTreeMap<SymbolId, Origins>,
    constraints: Vec<Constraint>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Base {
    Current,
    Unknown,
}
pub(super) struct RequireCall<'b, 'a> {
    pub arguments: &'b [Argument<'a>],
    pub base: Base,
    pub resolve_only: bool,
    pub reference: Option<u32>,
}

impl<'s> LoaderBindings<'s> {
    pub fn discover(program: &Program<'_>, scoping: &'s Scoping, current_file: &'s Path) -> Self {
        let mut result = Self {
            scoping,
            commonjs: program.source_type.is_script(),
            current_file,
            origins: BTreeMap::new(),
            constraints: Vec::new(),
        };
        for statement in &program.body {
            let Statement::ImportDeclaration(import) = statement else {
                continue;
            };
            if !matches!(import.source.value.as_str(), "module" | "node:module") {
                continue;
            }
            for specifier in import.specifiers.iter().flatten() {
                let (local, origin) = match specifier {
                    ImportDeclarationSpecifier::ImportSpecifier(specifier) => {
                        match specifier.imported.name().as_str() {
                            "createRequire" => (&specifier.local, Origin::Factory),
                            "default" => (&specifier.local, Origin::ModuleNamespace),
                            _ => continue,
                        }
                    }
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier) => {
                        (&specifier.local, Origin::ModuleNamespace)
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => {
                        (&specifier.local, Origin::ModuleNamespace)
                    }
                };
                if let Some(symbol) = local.symbol_id.get() {
                    result.origins.entry(symbol).or_default().insert(origin);
                }
            }
        }
        result.visit_program(program);
        let mut users = BTreeMap::<SymbolId, BTreeSet<usize>>::new();
        for (index, constraint) in result.constraints.iter().enumerate() {
            constraint.value.bindings(&mut |binding| {
                users.entry(binding).or_default().insert(index);
            });
        }
        // The lattice is the finite set of origins per lexical binding. Revisit
        // only dependent constraints when a binding gains an origin; no pass cap.
        let mut pending = (0..result.constraints.len()).collect::<BTreeSet<_>>();
        while let Some(index) = pending.pop_first() {
            let constraint = &result.constraints[index];
            let values = result.evaluate(&constraint.value);
            if values.is_empty() {
                continue;
            }
            let target = result.origins.entry(constraint.binding).or_default();
            let previous = target.len();
            target.extend(values);
            if target.len() != previous {
                pending.extend(
                    users
                        .get(&constraint.binding)
                        .into_iter()
                        .flatten()
                        .copied(),
                );
            }
        }
        result
    }

    fn symbol(&self, name: &IdentifierReference<'_>) -> Option<Option<SymbolId>> {
        let reference = self.scoping.get_reference(name.reference_id.get()?);
        (reference.is_value() && !reference.flags().is_value_as_type())
            .then_some(reference.symbol_id())
    }

    fn expression(&self, expression: &Expression<'_>) -> Option<Flow> {
        match expression.get_inner_expression() {
            Expression::Identifier(name) => match self.symbol(name)? {
                Some(symbol) => Some(Flow::Binding(symbol, name.span.start)),
                None if self.commonjs && name.name == "require" => {
                    Some(Flow::Root(Origin::RequireCurrent, name.span.start))
                }
                None if self.commonjs && name.name == "module" => {
                    Some(Flow::Root(Origin::ModuleRecord, name.span.start))
                }
                None => None,
            },
            Expression::StaticMemberExpression(member) => {
                let kind = match member.property.name.as_str() {
                    "createRequire" => Member::CreateRequire,
                    "constructor" => Member::Constructor,
                    "require" => Member::Require,
                    _ => return None,
                };
                Some(Flow::Member(
                    Box::new(self.expression(&member.object)?),
                    kind,
                ))
            }
            Expression::CallExpression(call) => {
                let input = match call.arguments.as_slice() {
                    [argument] => match argument
                        .as_expression()
                        .map(Expression::get_inner_expression)
                    {
                        Some(Expression::StaticMemberExpression(member))
                            if member.property.name == "url"
                                && matches!(
                                    member.object.get_inner_expression(),
                                    Expression::ImportMeta(_)
                                ) =>
                        {
                            Input::CurrentModuleBase
                        }
                        Some(Expression::StringLiteral(value))
                            if matches!(value.value.as_str(), "module" | "node:module") =>
                        {
                            Input::ModuleBuiltin
                        }
                        Some(Expression::StringLiteral(value))
                            if self.is_current_file(value.value.as_str()) =>
                        {
                            Input::CurrentModuleBase
                        }
                        _ => Input::Other,
                    },
                    _ => Input::Other,
                };
                Some(Flow::Call(Box::new(self.expression(&call.callee)?), input))
            }
            Expression::LogicalExpression(value) => {
                let left = self.expression(&value.left);
                let right = self.expression(&value.right);
                if value.operator == LogicalOperator::And {
                    // A tracked origin on the left is truthy, so only the right
                    // can supply the resulting loader. Keep guard uses for the
                    // passive-reference projection even when the result is unknown.
                    match left {
                        Some(left) => Some(Flow::And(Box::new(left), right.map(Box::new))),
                        None => right,
                    }
                } else {
                    Self::join(left, right)
                }
            }
            Expression::ConditionalExpression(value) => Self::join(
                self.expression(&value.consequent),
                self.expression(&value.alternate),
            ),
            _ => None,
        }
    }

    fn is_current_file(&self, value: &str) -> bool {
        Path::new(value) == self.current_file
            || url::Url::parse(value)
                .ok()
                .and_then(|url| url.to_file_path().ok())
                .is_some_and(|path| path == self.current_file)
    }

    fn join(left: Option<Flow>, right: Option<Flow>) -> Option<Flow> {
        match (left, right) {
            (Some(left), Some(right)) => Some(Flow::Join(Box::new(left), Box::new(right))),
            (one, None) | (None, one) => one,
        }
    }

    fn evaluate(&self, value: &Flow) -> Origins {
        match value {
            Flow::Binding(symbol, _) => self.origins.get(symbol).cloned().unwrap_or_default(),
            Flow::Root(origin, _) => BTreeSet::from([*origin]),
            Flow::And(_, right) => right
                .as_ref()
                .map(|right| self.evaluate(right))
                .unwrap_or_default(),
            Flow::Join(left, right) => {
                let mut values = self.evaluate(left);
                values.extend(self.evaluate(right));
                values
            }
            Flow::Member(object, member) => self
                .evaluate(object)
                .into_iter()
                .filter_map(|origin| match (origin, member) {
                    (Origin::ModuleNamespace, Member::CreateRequire) => Some(Origin::Factory),
                    (Origin::ModuleRecord, Member::Constructor) => Some(Origin::ModuleNamespace),
                    (Origin::ModuleRecord, Member::Require) => Some(Origin::RequireMethod),
                    _ => None,
                })
                .collect(),
            Flow::Call(callee, input) => self
                .evaluate(callee)
                .into_iter()
                .filter_map(|origin| match (origin, input) {
                    (Origin::Factory, Input::CurrentModuleBase) => Some(Origin::RequireCurrent),
                    (Origin::Factory, _) => Some(Origin::RequireUnknown),
                    (
                        Origin::RequireCurrent | Origin::RequireUnknown | Origin::RequireMethod,
                        Input::ModuleBuiltin,
                    ) => Some(Origin::ModuleNamespace),
                    _ => None,
                })
                .collect(),
        }
    }

    fn values(&self, expression: &Expression<'_>) -> Origins {
        self.expression(expression)
            .map(|value| self.evaluate(&value))
            .unwrap_or_default()
    }

    pub fn is_require_reference(&self, name: &IdentifierReference<'_>) -> bool {
        let Some(reference) = name.reference_id.get() else {
            return false;
        };
        if !self.scoping.get_reference(reference).is_read() {
            return false;
        }
        let values = match self.symbol(name) {
            Some(Some(symbol)) => self.origins.get(&symbol).cloned().unwrap_or_default(),
            Some(None) if self.commonjs && name.name == "require" => {
                BTreeSet::from([Origin::RequireCurrent])
            }
            _ => return false,
        };
        values.iter().any(|value| {
            matches!(
                value,
                Origin::RequireCurrent | Origin::RequireMethod | Origin::RequireUnknown
            )
        })
    }

    pub fn transfers(&self) -> BTreeSet<u32> {
        let mut positions = BTreeSet::new();
        for constraint in &self.constraints {
            self.transfer_positions(&constraint.value, &mut positions);
        }
        positions
    }

    fn transfer_positions(&self, value: &Flow, positions: &mut BTreeSet<u32>) {
        match value {
            Flow::Root(Origin::RequireCurrent, offset) => {
                positions.insert(*offset);
            }
            Flow::Binding(_, offset)
                if self.evaluate(value).iter().any(|value| {
                    matches!(
                        value,
                        Origin::RequireCurrent | Origin::RequireMethod | Origin::RequireUnknown
                    )
                }) =>
            {
                positions.insert(*offset);
            }
            Flow::Join(left, right) => {
                self.transfer_positions(left, positions);
                self.transfer_positions(right, positions);
            }
            Flow::And(left, right) => {
                self.transfer_positions(left, positions);
                if let Some(right) = right {
                    self.transfer_positions(right, positions);
                }
            }
            Flow::Member(object, _) => self.transfer_positions(object, positions),
            _ => {}
        }
    }

    pub fn require_call<'b, 'a>(
        &self,
        call: &'b CallExpression<'a>,
    ) -> Option<RequireCall<'b, 'a>> {
        let mut expression = call.callee.get_inner_expression();
        let mut arguments = call.arguments.as_slice();
        let mut receiver = None;
        let mut resolve_only = false;
        if let Expression::StaticMemberExpression(member) = expression {
            if member.property.name == "call" {
                receiver = arguments.first().and_then(Argument::as_expression);
                arguments = arguments.get(1..).unwrap_or_default();
                expression = member.object.get_inner_expression();
            } else if member.property.name == "resolve" {
                resolve_only = true;
                expression = member.object.get_inner_expression();
            }
        }
        let values = self.values(expression);
        let mut base = None;
        for value in values {
            let candidate = match value {
                Origin::RequireCurrent => Base::Current,
                Origin::RequireUnknown => Base::Unknown,
                Origin::RequireMethod => {
                    let receiver = receiver.or_else(|| match expression {
                        Expression::StaticMemberExpression(member) => Some(&member.object),
                        _ => None,
                    });
                    if receiver.is_some_and(|receiver| {
                        self.values(receiver).contains(&Origin::ModuleRecord)
                    }) {
                        Base::Current
                    } else {
                        Base::Unknown
                    }
                }
                _ => continue,
            };
            base = Some(if base == Some(Base::Unknown) {
                Base::Unknown
            } else {
                candidate
            });
        }
        let reference = match expression {
            Expression::Identifier(name) => Some(name.span.start),
            _ => None,
        };
        Some(RequireCall {
            arguments,
            base: base?,
            resolve_only,
            reference,
        })
    }
}

impl Flow {
    fn bindings(&self, visit: &mut impl FnMut(SymbolId)) {
        match self {
            Self::Binding(symbol, _) => visit(*symbol),
            Self::Member(value, _) | Self::Call(value, _) => value.bindings(visit),
            Self::Join(left, right) => {
                left.bindings(visit);
                right.bindings(visit);
            }
            Self::And(left, right) => {
                left.bindings(visit);
                if let Some(right) = right {
                    right.bindings(visit);
                }
            }
            Self::Root(..) => {}
        }
    }
}

impl<'a> Visit<'a> for LoaderBindings<'_> {
    fn visit_variable_declarator(&mut self, declaration: &VariableDeclarator<'a>) {
        if let BindingPattern::BindingIdentifier(name) = &declaration.id {
            if let Some(binding) = name.symbol_id.get() {
                // Top-level var declarations share the CommonJS wrapper's
                // parameters; a nested lexical shadow has a different symbol.
                if self.commonjs
                    && self.scoping.get_root_binding(name.name) == Some(binding)
                    && self
                        .scoping
                        .symbol_flags(binding)
                        .is_function_scoped_declaration()
                {
                    let origin = match name.name.as_str() {
                        "require" => Some(Origin::RequireCurrent),
                        "module" => Some(Origin::ModuleRecord),
                        _ => None,
                    };
                    if let Some(origin) = origin {
                        self.origins.entry(binding).or_default().insert(origin);
                    }
                }
                if let Some(value) = declaration
                    .init
                    .as_ref()
                    .and_then(|value| self.expression(value))
                {
                    self.constraints.push(Constraint { binding, value });
                }
            }
        }
        walk::walk_variable_declarator(self, declaration);
    }
    fn visit_assignment_expression(&mut self, assignment: &AssignmentExpression<'a>) {
        if assignment.operator == AssignmentOperator::Assign {
            if let AssignmentTarget::AssignmentTargetIdentifier(name) = &assignment.left {
                if let (Some(Some(binding)), Some(value)) =
                    (self.symbol(name), self.expression(&assignment.right))
                {
                    self.constraints.push(Constraint { binding, value });
                }
            }
        }
        walk::walk_assignment_expression(self, assignment);
    }
}

#[cfg(test)]
mod tests {
    use crate::aot::{DependencyGraph, GraphRequirement, SourceManifest};
    use std::{fs, path::Path};

    fn fixture(
        name: &str,
        extension: &str,
        source: &str,
        check: impl FnOnce(&DependencyGraph, &Path),
    ) {
        let directory = std::env::temp_dir().join(format!(
            "quench-loader-provenance-{}-{name}-{extension}",
            std::process::id()
        ));
        fs::create_dir(&directory).unwrap();
        let entry = directory.join(format!("entry.{extension}"));
        fs::write(&entry, source).unwrap();
        fs::write(directory.join("dependency.cjs"), "module.exports = 42;").unwrap();
        fs::write(directory.join("invalid.cjs"), "let invalid = ;").unwrap();
        let graph = SourceManifest::default().add_module_graph(&entry).unwrap();
        check(&graph, &entry.canonicalize().unwrap());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn loader_factory_aliases_preserve_runtime_provenance_through_false_ts_assertions() {
        let source = r#"
            import { createRequire as create } from 'node:module';
            const factory = create;
            let loader; loader = factory(import.meta.url);
            const again = loader;
            again.resolve('./dependency.cjs');
            again('./dependency.cjs');
            const mod = again('module');
            const other = mod.createRequire(import.meta.url);
            other('./dependency.cjs');
        "#;
        for extension in ["mjs", "mts"] {
            let source = if extension == "mts" {
                source.replace(
                    "const factory = create;",
                    "const factory = (create as unknown as () => string);",
                )
            } else {
                source.into()
            };
            fixture("factories", extension, &source, |graph, entry| {
                assert!(graph.requirements.is_empty(), "{:?}", graph.requirements);
                assert!(graph
                    .files
                    .contains_key(&entry.with_file_name("dependency.cjs")));
            });
        }
    }

    #[test]
    fn commonjs_loader_and_module_method_aliases_share_the_same_resolution() {
        fixture(
            "commonjs",
            "cjs",
            r#"
            const first = require;
            let second; second = first;
            second('./dependency.cjs');
            const method = module && module.require;
            const copy = method;
            copy.call(module, './dependency.cjs');
        "#,
            |graph, entry| {
                assert!(graph.requirements.is_empty(), "{:?}", graph.requirements);
                assert_eq!(graph.files.len(), 2);
                assert_eq!(
                    graph.resolutions[&(entry.to_owned(), "./dependency.cjs".into())],
                    entry.with_file_name("dependency.cjs")
                );
            },
        );
    }

    #[test]
    fn shadowed_names_and_asserted_functions_do_not_become_node_loaders() {
        fixture(
            "shadowed",
            "mts",
            r#"
            function local(require: any, module: any, createRequire: any) {
                const factory = createRequire;
                const loader = factory(import.meta.url);
                const method = module.require;
                require('./missing.cjs'); loader('./missing.cjs'); method.call(module, './missing.cjs');
            }
            const fake = (() => () => 42) as unknown as typeof import('node:module').createRequire;
            const loader = fake(import.meta.url);
            loader('./missing.cjs');
        "#,
            |graph, _| {
                assert!(graph.requirements.is_empty(), "{:?}", graph.requirements);
                assert_eq!(graph.files.len(), 1);
            },
        );
    }

    #[test]
    fn unknown_loader_bases_and_computed_requests_remain_explicit() {
        fixture("unknown-base", "mjs", "import { createRequire } from 'node:module'; const loader = createRequire(process.argv[2]); loader('./dependency.cjs');", |graph, _| {
            assert_eq!(graph.requirements.len(), 1);
            assert!(matches!(graph.requirements[0], GraphRequirement::DynamicRequire { .. }));
        });
        fixture(
            "computed",
            "cjs",
            "const loader = require; loader(process.argv[2]);",
            |graph, _| {
                assert_eq!(graph.requirements.len(), 1);
                assert!(matches!(
                    graph.requirements[0],
                    GraphRequirement::DynamicRequire { .. }
                ));
            },
        );
    }

    #[test]
    fn resolving_a_dependency_does_not_parse_or_archive_its_body() {
        fixture("resolve-only", "mjs", "import { createRequire as make } from 'module'; const loader = make(import.meta.url); export const path = loader.resolve('./invalid.cjs');", |graph, entry| {
            assert!(graph.requirements.is_empty(), "{:?}", graph.requirements);
            assert!(!graph.files.contains_key(&entry.with_file_name("invalid.cjs")));
            assert_eq!(graph.resolutions[&(entry.to_owned(), "./invalid.cjs".into())], entry.with_file_name("invalid.cjs"));
        });
    }
    #[test]
    fn truthy_module_guards_do_not_turn_loaded_export_values_into_loaders() {
        for extension in ["cjs", "cts"] {
            fixture(
                "truthy-module",
                extension,
                r#"
                const record = typeof module === 'object' && module && !module.nodeType && module;
                const types = record && record.require && record.require('node:util').types;
                if (types) types.isTypedArray(new Uint8Array());
                const loader = require;
                loader && loader('./dependency.cjs');
                const result = loader && loader('./dependency.cjs');
                if (result) console.log(result);
            "#,
                |graph, _| {
                    assert!(graph.requirements.is_empty(), "{:?}", graph.requirements);
                    assert!(graph.builtins.contains("node:util"));
                    assert_eq!(graph.files.len(), 2);
                },
            );
        }
    }
    #[test]
    fn logical_or_can_return_a_loader_and_keeps_its_escape_requirement() {
        fixture(
            "logical-or-escape",
            "cjs",
            "const loader = require; module.exports = loader || (() => 42);",
            |graph, _| {
                assert_eq!(graph.requirements.len(), 1);
                assert!(matches!(
                    graph.requirements[0],
                    GraphRequirement::DynamicRequire { .. }
                ));
            },
        );
    }
}
