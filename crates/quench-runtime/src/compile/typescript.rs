//! TypeScript is a source language of the engine: the compiler admits the typed tree
//! (`admit`) and keeps its declarations. A declared type is no run-time proof (TypeScript
//! is unsound), so tiers check it once where a value enters typed code, then trust it.

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_span::Span;

mod admit;
mod enums;
mod imports;
pub(super) use admit::{Declarations, DeclaredKind};
pub(super) use imports::{elide as elide_type_only_imports, type_only_imports};
pub(super) use admit::admit;

/// What admission leaves without a run-time meaning yet: decorators.
pub(super) fn unsupported_runtime_syntax(program: &Program<'_>) -> Option<(Span, &'static str)> {
    #[derive(Default)]
    struct Check {
        error: Option<(Span, &'static str)>,
    }

    impl Check {
        fn decorated(&mut self, decorators: &[Decorator<'_>], span: Span) {
            if !decorators.is_empty() {
                self.error.get_or_insert((span, "TypeScript decorators need a runtime meaning"));
            }
        }
    }

    impl<'a> Visit<'a> for Check {
        fn visit_formal_parameter(&mut self, parameter: &FormalParameter<'a>) {
            self.decorated(&parameter.decorators, parameter.span);
            walk::walk_formal_parameter(self, parameter);
        }

        fn visit_class(&mut self, class: &Class<'a>) {
            self.decorated(&class.decorators, class.span);
            walk::walk_class(self, class);
        }

        fn visit_property_definition(&mut self, property: &PropertyDefinition<'a>) {
            self.decorated(&property.decorators, property.span);
            walk::walk_property_definition(self, property);
        }

        fn visit_accessor_property(&mut self, accessor: &AccessorProperty<'a>) {
            self.decorated(&accessor.decorators, accessor.span);
            walk::walk_accessor_property(self, accessor);
        }

        fn visit_method_definition(&mut self, method: &MethodDefinition<'a>) {
            self.decorated(&method.decorators, method.span);
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

    fn run_typescript(source: &str) -> Vec<String> {
        let mut outputs = Vec::new();
        for mode in [ExecutionMode::Interpreted, ExecutionMode::Compiled] {
            let program = Engine::specialize_typescript(source, "admitted.cts").unwrap();
            let host = TestHost::default();
            let view = host.clone();
            let mut runtime = Runtime::new(host);
            runtime.set_execution_mode(mode);
            runtime.execute(&program).unwrap();
            outputs.push(view.lines().join("|"));
        }
        assert_eq!(outputs[0], outputs[1]);
        outputs.swap_remove(0).split('|').map(str::to_owned).collect()
    }

    #[test]
    fn declarations_without_runtime_meaning_are_admitted() {
        let lines = run_typescript(r#"
            interface Point { x: number }
            type Alias = Point | string;
            declare const ambient: number;
            declare function external(): void;
            declare namespace Types { const t: number; }
            function pick(value: string): string;
            function pick(value: number): number;
            function pick(value: any) { return value; }
            abstract class Base {
                abstract area(): number;
                abstract readonly name: string;
                declare tag: string;
                [key: string]: unknown;
                describe() { return this.area(); }
            }
            class Square extends Base { name = "sq"; constructor(private side: number) { super(); } area(): number; area() { return this.side ** 2; } }
            function local() { interface Inner { y: number } type T = number; const v: T = 2; return v; }
            console.log([pick(1), pick("s"), new Square(3).describe(), local(), "tag" in new Square(1)].join(" "));
        "#);
        assert_eq!(lines, ["1 s 9 2 false"]);
    }

    #[test]
    fn enums_namespaces_and_parameter_properties_have_their_runtime_meaning() {
        let lines = run_typescript(r#"
            enum E { A, B, C = A + 10, D = "d" }
            const enum Flags { X = 1 << 3, Y = X | 1 }
            enum Computed { Len = "abc".length, Next }
            enum E { Late = 20 }
            namespace N { export const k = 1; export function g() { return k + 1; } export namespace M { export enum Q { R } } }
            namespace N { export const k2 = 2; }
            namespace A.B.C { export const deep = "deep"; }
            class P { constructor(public x: number, protected readonly y = 2, z?: number) {} sum() { return this.x + this.y; } }
            class Q extends P { extra = 5; constructor(x: number, private w: string) { super(x); } }
            const q = new Q(1, "w");
            import alias = N.M;
            console.log([E.A, E[1], E.C, E[10], E.D, E.Late, E[20], Flags.Y, Computed.Len, Computed.Next, Computed[4]].join(" "));
            console.log([N.k, N.g(), N.M.Q.R, N.M.Q[0], N.k2, A.B.C.deep, alias.Q.R].join(" "));
            console.log([new P(3).sum(), q.sum(), Object.keys(q).join()].join(" "));
        "#);
        assert_eq!(lines, [
            "0 B 10 C d 20 Late 9 3 4 Next",
            "1 2 0 R 2 deep 0",
            "5 3 x,y,extra,w",
        ]);
    }

    #[test]
    fn decorators_still_need_a_runtime_meaning() {
        for source in ["@sealed class Box {}", "class Box { @log method() {} }", "class Box { constructor(@inject() value: number) {} }"] {
            assert!(Engine::specialize_typescript(source, "decorated.cts").is_err(), "{source}");
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
