//! Admit typed syntax and keep its declarations as facts. A declaration is no runtime proof
//! (TypeScript is unsound: `any`, assertions), so a declared type is a seed the tiers check
//! once where a value enters typed code, then trust (`Function::declared_parameters`).
//! Runtime-changing TS constructs must have a lowering before admission.

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_span::Span;
use oxc_syntax::scope::ScopeFlags;

/// Remove only erased module declarations/specifiers. Empty value imports and
/// reexports retain their evaluation edge, even when all named types disappear.
pub(super) fn erase_module_types(program: &mut Program<'_>) {
    program.body.retain_mut(|statement| match statement {
        Statement::TSTypeAliasDeclaration(_) | Statement::TSInterfaceDeclaration(_) => false,
        Statement::ImportDeclaration(import) => {
            if import.import_kind.is_type() { return false; }
            if let Some(specifiers) = &mut import.specifiers {
                specifiers.retain(|specifier| !matches!(specifier,
                    ImportDeclarationSpecifier::ImportSpecifier(specifier) if specifier.import_kind.is_type()));
            }
            true
        },
        Statement::ExportNamedDeclaration(export) => {
            if export.export_kind.is_type() { return false; }
            export.specifiers.retain(|specifier| !specifier.export_kind.is_type());
            true
        },
        Statement::ExportFromDeclaration(export) => {
            if export.export_kind.is_type() { return false; }
            export.specifiers.retain(|specifier| !specifier.export_kind.is_type());
            true
        },
        Statement::ExportAllDeclaration(export) => !export.export_kind.is_type(),
        Statement::ExportDeclaration(export) => !matches!(export.declaration,
            Declaration::TSTypeAliasDeclaration(_) | Declaration::TSInterfaceDeclaration(_)),
        Statement::ExportDefaultDeclaration(export) => !matches!(export.declaration,
            ExportDefaultDeclarationKind::TSInterfaceDeclaration(_)),
        _ => true,
    });
}

/// The language types (`crate::typed_ir::Type::language` bits) each formal parameter of
/// `formals` declares, by argument index; empty where none declares one. An undeclared or
/// structural type (an interface, a type literal, a generic) is `DECLARED_ANY`, as a
/// structural type admits primitives too. An optional parameter or one with a default may
/// enter as `undefined`.
pub(super) fn declared_parameters(formals: &FormalParameters<'_>) -> Vec<u16> {
    if formals.items.iter().all(|item| item.type_annotation.is_none()) {
        return Vec::new();
    }
    formals.items.iter().map(|item| match &item.type_annotation {
        Some(annotation) if item.optional || item.initializer.is_some() =>
            declared_language(&annotation.type_annotation) | language(crate::ValueType::Undefined),
        Some(annotation) => declared_language(&annotation.type_annotation),
        None => DECLARED_ANY,
    }).collect()
}

/// No declared fact: every language type.
pub(crate) const DECLARED_ANY: u16 = u16::MAX;

const fn language(kind: crate::ValueType) -> u16 {
    1 << kind as u16
}

/// The language types a TypeScript type admits at run time, `DECLARED_ANY` where it does not
/// name a closed set.
fn declared_language(ty: &TSType<'_>) -> u16 {
    use crate::ValueType as V;
    match ty {
        TSType::TSNumberKeyword(_) => language(V::Number),
        TSType::TSStringKeyword(_) | TSType::TSTemplateLiteralType(_) => language(V::String),
        TSType::TSBooleanKeyword(_) => language(V::Boolean),
        TSType::TSBigIntKeyword(_) => language(V::BigInt),
        TSType::TSSymbolKeyword(_) => language(V::Symbol),
        TSType::TSUndefinedKeyword(_) | TSType::TSVoidKeyword(_) => language(V::Undefined),
        TSType::TSNullKeyword(_) => language(V::Null),
        TSType::TSObjectKeyword(_) => language(V::Object) | language(V::Function),
        TSType::TSArrayType(_) | TSType::TSTupleType(_) => language(V::Object),
        TSType::TSFunctionType(_) | TSType::TSConstructorType(_) => language(V::Function),
        TSType::TSLiteralType(literal) => match &literal.literal {
            TSLiteral::NumericLiteral(_) => language(V::Number),
            TSLiteral::StringLiteral(_) | TSLiteral::TemplateLiteral(_) => language(V::String),
            TSLiteral::BooleanLiteral(_) => language(V::Boolean),
            TSLiteral::BigIntLiteral(_) => language(V::BigInt),
            TSLiteral::UnaryExpression(_) => language(V::Number) | language(V::BigInt),
        },
        TSType::TSParenthesizedType(inner) => declared_language(&inner.type_annotation),
        TSType::TSUnionType(union) => union.types.iter().fold(0, |set, member| set | declared_language(member)),
        _ => DECLARED_ANY,
    }
}

pub(super) fn unsupported_runtime_syntax(program: &Program<'_>) -> Option<(Span, &'static str)> {
    #[derive(Default)]
    struct Check {
        error: Option<(Span, &'static str)>,
    }

    impl Check {
        fn reject(&mut self, span: Span, message: &'static str) {
            self.error.get_or_insert((span, message));
        }
    }

    impl<'a> Visit<'a> for Check {
        fn visit_formal_parameter(&mut self, parameter: &FormalParameter<'a>) {
            if parameter.accessibility.is_some() || parameter.readonly || parameter.r#override {
                self.reject(
                    parameter.span,
                    "TypeScript parameter properties need a runtime lowering",
                );
            }
            if !parameter.decorators.is_empty() {
                self.reject(
                    parameter.span,
                    "TypeScript parameter decorators need a runtime lowering",
                );
            }
            walk::walk_formal_parameter(self, parameter);
        }

        fn visit_variable_declaration(&mut self, declaration: &VariableDeclaration<'a>) {
            if declaration.declare {
                self.reject(
                    declaration.span,
                    "ambient declarations need type-only binding lowering",
                );
            }
            walk::walk_variable_declaration(self, declaration);
        }

        fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
            if function.declare || function.body.is_none() {
                self.reject(
                    function.span,
                    "ambient functions and overloads need type-only binding lowering",
                );
            }
            walk::walk_function(self, function, flags);
        }

        fn visit_class(&mut self, class: &Class<'a>) {
            if class.declare || !class.decorators.is_empty() {
                self.reject(class.span, "ambient classes and decorators need a lowering");
            }
            walk::walk_class(self, class);
        }

        fn visit_property_definition(&mut self, property: &PropertyDefinition<'a>) {
            if property.declare || !property.decorators.is_empty() {
                self.reject(
                    property.span,
                    "declared class fields and decorators need a lowering",
                );
            }
            walk::walk_property_definition(self, property);
        }

        fn visit_method_definition(&mut self, method: &MethodDefinition<'a>) {
            if !method.decorators.is_empty() {
                self.reject(method.span, "decorated methods need a runtime lowering");
            }
            walk::walk_method_definition(self, method);
        }
    }

    let mut check = Check::default();
    check.visit_program(program);
    check.error
}

#[cfg(test)]
mod tests {
    use crate::{Engine, ExecutionMode, Runtime, test_host::TestHost};

    #[test]
    fn typescript_annotations_and_assertions_do_not_override_runtime_values() {
        let source = r#"
            type Box<T> = { value: T };
            interface Item { readonly value: number }
            function identity<T>(value: T): T { return value; }
            function add(value: number): number { return value + 1; }
            const item: Item = { value: 7 };
            (item as any).value = 'changed';
            for (let i = 0; i < 256; i++) add(i);
            console.log(item.value + 1);
            console.log(add(('text' as unknown) as number));
            console.log(identity<string>('typed'));
            console.log((5 satisfies number) + 1);
        "#;
        for mode in [ExecutionMode::Interpreted, ExecutionMode::Compiled] {
            let program = Engine::specialize_typescript(source, "typed.cts").unwrap();
            let bytes: crate::KeptBytes = std::rc::Rc::new(program.encode_trusted());
            let program = crate::ResidualProgram::decode_trusted(bytes).unwrap();
            let host = TestHost::default();
            let view = host.clone();
            let mut runtime = Runtime::new(host);
            runtime.set_execution_mode(mode);
            runtime.execute(&program).unwrap();
            assert_eq!(&*view.lines(), &["changed1", "text1", "typed", "6"]);
        }
    }

    #[test]
    fn type_only_wrappers_keep_references_and_names() {
        // TypeScript erases `!`, `as`, `satisfies` and `<T>` at run time: a call through one
        // keeps `this`, `typeof` through one tolerates a missing name, and an anonymous
        // function under one takes its binding's name.
        let source = r#"
            const obj = { v: 7, m() { return this?.v; } };
            console.log(obj.m!(), (obj.m as any)(), (<any>obj.m)(), (obj.m satisfies Function)(), obj['m']!());
            console.log(typeof (missingName as any), typeof missingName!, typeof (<any>missingName));
            const f = (() => 1) as () => number;
            const g = (function () {}) satisfies Function;
            console.log(f.name, g.name);
        "#;
        for mode in [ExecutionMode::Interpreted, ExecutionMode::Compiled] {
            let program = Engine::specialize_typescript(source, "wrappers.cts").unwrap();
            let host = TestHost::default();
            let view = host.clone();
            let mut runtime = Runtime::new(host);
            runtime.set_execution_mode(mode);
            runtime.execute(&program).unwrap();
            assert_eq!(&*view.lines(), &["7 7 7 7 7", "undefined undefined undefined", "f g"]);
        }
    }

    #[test]
    fn declared_parameter_types_are_kept_as_facts() {
        let source = "function add(a: number, c: Shape, d: number | bigint, e, b?: string) { return a; }\nconst f = (x: boolean) => x;\nfunction g(p, q) { return p; }";
        let program = Engine::specialize_typescript(source, "declared.cts").unwrap();
        let declared = |name: &str| (0..program.functions.len()).filter_map(|index| program.function_get(index))
            .find(|function| function.name.is_some_and(|atom| program.atoms.get(atom as usize) == Some(name)))
            .map(|function| function.declared_parameters.to_vec());
        use crate::ValueType as V;
        let bit = |kind: V| 1_u16 << kind as u16;
        assert_eq!(declared("add").unwrap(), vec![bit(V::Number), super::DECLARED_ANY,
            bit(V::Number) | bit(V::BigInt), super::DECLARED_ANY, bit(V::String) | bit(V::Undefined)]);
        assert_eq!(declared("g").unwrap(), Vec::<u16>::new());
    }

    #[test]
    fn javascript_parse_goal_still_rejects_type_annotations() {
        assert!(Engine::specialize("const value: number = 1;", "named.cts").is_err());
    }

    #[test]
    fn unsupported_typescript_cannot_silently_drop_runtime_effects() {
        for source in [
            "class Box { constructor(public value: number) {} }",
            "function deferred() { class Box { constructor(readonly value: number) {} } }",
            "declare const absent: number;",
            "declare function absent(): number;",
            "class Box { declare value: number; }",
            "enum Color { Red, Green }",
        ] {
            assert!(
                Engine::specialize_typescript(source, "unsupported.cts").is_err(),
                "{source}"
            );
        }
    }
}

#[cfg(test)]
mod module_tests {
    use crate::Engine;

    #[test]
    fn typed_module_analysis_does_not_share_javascript_cache_entries() {
        let source = "export type Shape = number; export const value: number = 1;";
        for typed_first in [false, true] {
            let name = if typed_first { "typed-first" } else { "javascript-first" };
            for typescript in [typed_first, !typed_first, typed_first] {
                let analysis = Engine::static_module_analysis_with(source, name, typescript, |analyze| analyze());
                assert_eq!(analysis.early_error.is_none(), typescript);
                assert_eq!(analysis.plan.is_some(), typescript);
            }
        }
    }

    #[test]
    fn erased_module_projection_preserves_unicode_offsets_and_runtime_edges() {
        let source = "export interface Café { value: '🙂' }\u{2028}export type Alias = string;\r\nconst value: number = 1;";
        let (projected, has_module_declarations) = Engine::project_typescript_module_syntax(source).unwrap();
        assert!(!has_module_declarations);
        assert_eq!(projected.len(), source.len());
        let suffix = "const value: number = 1;";
        assert_eq!(projected.find(suffix), source.find(suffix));
        assert_eq!(projected.chars().filter(|character| oxc_syntax::line_terminator::is_line_terminator(*character)).collect::<String>(), "\u{2028}\r\n");
        for source in ["import { type Shape } from './side.mts';", "export { type Shape } from './side.mts';"] {
            let (_, has_module_declarations) = Engine::project_typescript_module_syntax(source).unwrap();
            assert!(has_module_declarations);
        }
        assert!(Engine::project_typescript_module_syntax("export interface {").is_none());
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_module_key_keeps_language_in_its_compile_goal() {
        let source = "export const value: number = 1;";
        let mut compiler = crate::NativeJsCompiler::new(true);
        let javascript = crate::NativeSourceKey::new("same-name", &["specialized", crate::STATIC_MODULE_GOAL, source]).unwrap();
        let typed = crate::NativeSourceKey::new("same-name", &["specialized", crate::STATIC_TYPESCRIPT_MODULE_GOAL, source]).unwrap();
        assert!(compiler.compile_source(&javascript).is_err());
        assert!(compiler.compile_source(&typed).is_ok());
    }
}
