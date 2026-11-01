//! Admit typed syntax without assuming declarations are runtime proofs.
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
