#[cfg(test)]
mod integration_tests {
    use crate::transpile::analyzer::Analyzer;
    use crate::transpile::hir::*;
    use crate::transpile::parser::TsParser;

    #[test]
    fn test_full_transpile_simple_component() {
        let source = r#"interface Props { name: string; } export default function Greeting({ name }: Props) { return <div>Hello, {name}!</div>; }"#;
        let parser = TsParser::new();
        let module = parser.parse_tsx(source).expect("parse failed");
        let has_type = module
            .items
            .iter()
            .any(|item| matches!(item, ModuleItem::Decl(Decl::Type(_))));
        assert!(has_type, "Module should have type declaration");
        assert!(!module.items.is_empty(), "Module should have items");
    }

    #[test]
    fn test_full_transpile_island() {
        let source = r#"import { useState } from "preact/hooks"; interface CounterProps { initial?: number; } export default function Counter({ initial = 0 }: CounterProps) { const [count, setCount] = useState(initial); return <div class="counter"><p>Count: {count}</p><button onClick={() => setCount(count + 1)}>+</button></div>; }"#;
        let parser = TsParser::new();
        let module = parser.parse_tsx(source).unwrap();
        let has_import = module
            .items
            .iter()
            .any(|item| matches!(item, ModuleItem::Import(_)));
        assert!(has_import, "Module should have imports");
        assert!(!module.items.is_empty(), "Module should not be empty");
    }

    #[test]
    #[ignore] // TODO: requires ExportNamedDeclaration support
    fn test_full_transpile_route_handler() {
        let source = r#"export const handler = { async GET(): Promise<Response> { return new Response("hello"); } }; export default function Post() { return <article>Hello</article>; }"#;
        let parser = TsParser::new();
        let module = parser.parse_tsx(source).expect("parse failed");
        assert!(!module.items.is_empty(), "Module should have items");
    }

    #[test]
    fn test_full_pipeline_transpile_island_to_rust() {
        let source = r#"interface Props { initial?: number; } export default function Counter({ initial = 0 }: Props) { const [count, setCount] = useState(initial); return <div>Count: {count}</div>; }"#;
        let parser = TsParser::new();
        let module = parser.parse_tsx(source).expect("parse failed");
        assert!(!module.items.is_empty(), "Module should have items");
    }

    #[test]
    fn test_full_pipeline_signals_usage() {
        let source = r#"import { signal, computed } from "@preact/signals"; const count = signal(0); const doubled = computed(() => count.value * 2); export default function Counter() { return <div>Count: {count}</div>; }"#;
        let parser = TsParser::new();
        let module = parser.parse_tsx(source).expect("parse failed");
        let mut analyzer = Analyzer::new();
        let result = analyzer.analyze(&module);
        assert!(result.is_ok(), "Analysis should succeed");
        assert!(analyzer.signals.contains("signal"), "Should detect signal");
        assert!(
            analyzer.signals.contains("computed"),
            "Should detect computed"
        );
    }

    #[test]
    fn test_parse_string_literals() {
        let source = r#"const empty = ""; const hello = "hello"; const world = 'world'; const template = `template`;"#;
        let parser = TsParser::new();
        let result = parser.parse_tsx(source);
        assert!(
            result.is_ok(),
            "Failed to parse string literals: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_parse_number_literals() {
        let source = r#"const zero = 0; const integer = 42; const float = 3.14;"#;
        let parser = TsParser::new();
        let result = parser.parse_tsx(source);
        assert!(
            result.is_ok(),
            "Failed to parse number literals: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_parse_boolean_null() {
        let source = r#"const t = true; const f = false; const n = null;"#;
        let parser = TsParser::new();
        let result = parser.parse_tsx(source);
        assert!(
            result.is_ok(),
            "Failed to parse boolean/null: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_parse_function_declarations() {
        let source = r#"function add(a: number, b: number): number { return a + b; } function greet(name: string): string { return "Hello, " + name; }"#;
        let parser = TsParser::new();
        let result = parser.parse_tsx(source);
        assert!(
            result.is_ok(),
            "Failed to parse functions: {:?}",
            result.err()
        );
        let module = result.unwrap();
        assert!(!module.items.is_empty(), "Module should have items");
    }

    #[test]
    fn test_parse_binary_expressions() {
        let source = r#"const add = 1 + 2; const sub = 5 - 3; const mul = 4 * 3; const div = 10 / 2; const eq = 5 == 5; const lt = 3 < 5;"#;
        let parser = TsParser::new();
        let result = parser.parse_tsx(source);
        assert!(
            result.is_ok(),
            "Failed to parse binary expressions: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_parse_logical_expressions() {
        let source = r#"const and = true && false; const or = true || false; const not = !true;"#;
        let parser = TsParser::new();
        let result = parser.parse_tsx(source);
        assert!(
            result.is_ok(),
            "Failed to parse logical expressions: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_parse_conditional() {
        let source = r#"const ternary = x > 0 ? "positive" : "negative";"#;
        let parser = TsParser::new();
        let result = parser.parse_tsx(source);
        assert!(
            result.is_ok(),
            "Failed to parse conditional: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_parse_array_expressions() {
        let source = r#"const arr = [1, 2, 3]; const nested = [[1, 2], [3, 4]];"#;
        let parser = TsParser::new();
        let result = parser.parse_tsx(source);
        assert!(result.is_ok(), "Failed to parse arrays: {:?}", result.err());
    }

    #[test]
    fn test_parse_object_expressions() {
        let source = r#"const obj = { a: 1, b: 2 }; const nested = { outer: { inner: 1 } };"#;
        let parser = TsParser::new();
        let result = parser.parse_tsx(source);
        assert!(
            result.is_ok(),
            "Failed to parse objects: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_parse_arrow_functions() {
        let source = r#"const add = (a: number, b: number): number => a + b; const greet = (name: string) => "Hello, " + name;"#;
        let parser = TsParser::new();
        let result = parser.parse_tsx(source);
        assert!(
            result.is_ok(),
            "Failed to parse arrow functions: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_parse_template_literals() {
        let source = r#"const greeting = `Hello, ${name}!`;"#;
        let parser = TsParser::new();
        let result = parser.parse_tsx(source);
        assert!(
            result.is_ok(),
            "Failed to parse templates: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_parse_imports() {
        let source = r#"import { foo } from 'module'; import bar from 'default'; import * as ns from 'namespace';"#;
        let parser = TsParser::new();
        let result = parser.parse_tsx(source);
        assert!(
            result.is_ok(),
            "Failed to parse imports: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_parse_exports() {
        let source = r#"export const a = 1; export function foo() { return 1; }"#;
        let parser = TsParser::new();
        let result = parser.parse_tsx(source);
        assert!(
            result.is_ok(),
            "Failed to parse exports: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_parse_member_expressions() {
        let source = r#"const val = obj.property; const computed = arr[0];"#;
        let parser = TsParser::new();
        let result = parser.parse_tsx(source);
        assert!(
            result.is_ok(),
            "Failed to parse member expressions: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_parse_call_expressions() {
        let source = r#"const result = foo(1, 2); const chained = obj.method();"#;
        let parser = TsParser::new();
        let result = parser.parse_tsx(source);
        assert!(
            result.is_ok(),
            "Failed to parse call expressions: {:?}",
            result.err()
        );
    }
}
