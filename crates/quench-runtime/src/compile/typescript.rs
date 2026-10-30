//! Admit typed syntax without assuming declarations are runtime proofs.
//! Runtime-changing TS constructs must have a lowering before admission.

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_span::Span;
use oxc_syntax::scope::ScopeFlags;

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
