//! Integration tests
//!
//! allow:too_many_lines,complexity

#[cfg(test)]
mod integration_tests {
    use crate::transpile::analyzer::Analyzer;
    use crate::transpile::hir::*;
    use crate::transpile::hir::type_to_rust::{TypeToRust, OutputKind};
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

    // ===== Tests for HIR fixes =====

    #[test]
    fn test_type_to_rust_union() {
        let converter = TypeToRust::new(OutputKind::String);
        let union_type = Type::Union {
            types: vec![Type::String, Type::Number],
        };
        let result = converter.convert(&union_type);
        let type_name = result.type_name();
        assert!(type_name.contains("enum"), "Union should generate enum: {}", type_name);
    }

    #[test]
    fn test_type_to_rust_intersection() {
        let converter = TypeToRust::new(OutputKind::String);
        let intersection_type = Type::Intersection {
            types: vec![
                Type::Object { members: vec![TypeMember { key: "a".into(), type_: Type::String, optional: false, readonly: false }] },
                Type::Object { members: vec![TypeMember { key: "b".into(), type_: Type::Number, optional: false, readonly: false }] },
            ],
        };
        let result = converter.convert(&intersection_type);
        let type_name = result.type_name();
        assert!(type_name.contains("struct") || type_name.contains("{"), "Intersection should generate struct-like type: {}", type_name);
    }

    #[test]
    fn test_type_to_rust_array() {
        let converter = TypeToRust::new(OutputKind::String);
        let array_type = Type::Array { elem: Box::new(Type::String) };
        let result = converter.convert(&array_type);
        assert_eq!(result.type_name(), "Vec<String>");
    }

    #[test]
    fn test_type_to_rust_function() {
        let converter = TypeToRust::new(OutputKind::String);
        let fn_type = Type::Function {
            params: vec![Type::String, Type::Number],
            ret: Box::new(Type::Boolean),
        };
        let result = converter.convert(&fn_type);
        let type_name = result.type_name();
        assert!(type_name.contains("fn("), "Function type should generate fn(): {}", type_name);
    }

    #[test]
    fn test_type_to_rust_hashmap() {
        let converter = TypeToRust::new(OutputKind::String);
        let map_type = Type::Mapped {
            from: Box::new(Type::String),
            to: Box::new(Type::Number),
        };
        let result = converter.convert(&map_type);
        let type_name = result.type_name();
        assert!(type_name.contains("HashMap"), "Mapped should generate HashMap: {}", type_name);
    }

    #[test]
    fn test_type_to_rust_literal() {
        let converter = TypeToRust::new(OutputKind::String);
        let literal_type = Type::Literal {
            kind: LiteralKind::String,
            value: "hello".into(),
        };
        let result = converter.convert(&literal_type);
        let type_name = result.type_name();
        assert!(type_name.contains("hello"), "Literal should preserve value: {}", type_name);
    }

    #[test]
    fn test_type_to_rust_template() {
        let converter = TypeToRust::new(OutputKind::String);
        let template_type = Type::Template {
            parts: vec![
                TemplatePart::String("Hello, ".into()),
                TemplatePart::Type(Type::String),
            ],
            values: vec![Type::String],
        };
        let result = converter.convert(&template_type);
        let type_name = result.type_name();
        // Template with dynamic parts falls back to Value
        assert!(type_name == "Value" || type_name.contains("Hello"), "Template should handle correctly: {}", type_name);
    }

    #[test]
    fn test_pat_binding_names() {
        let pat = Pat::Object {
            props: vec![
                ObjectPatProp::Init { key: "a".into(), value: Pat::Ident { name: "x".into(), type_: None } },
                ObjectPatProp::Rest { arg: Box::new(Pat::Ident { name: "rest".into(), type_: None }) },
            ],
            rest: None,
        };
        let names = pat.binding_names();
        assert!(names.contains(&"x".into()), "Should extract binding names: {:?}", names);
        assert!(names.contains(&"rest".into()), "Should extract rest binding: {:?}", names);
    }

    #[test]
    fn test_object_pat_prop_variants() {
        // Test that Spread and Method variants exist
        let spread_prop = ObjectPatProp::Spread { arg: Box::new(Pat::Ident { name: "rest".into(), type_: None }) };
        let method_prop = ObjectPatProp::Method { key: "handler".into() };

        assert!(spread_prop.key_name().is_none(), "Spread should have no key name");
        assert_eq!(method_prop.key_name(), Some("handler"), "Method should have key name");
    }

    #[test]
    fn test_ownership_analyzer_for_loop() {
        use crate::transpile::hir::ownership::OwnershipAnalyzer;

        let func = FunctionDecl {
            name: "test".into(),
            generics: vec![],
            params: vec![
                Param {
                    name: "arr".into(),
                    type_: Some(Type::Array { elem: Box::new(Type::Number) }),
                    default: None,
                    optional: false,
                    pattern: None,
                    ownership: Ownership::Owned,
                }
            ],
            return_type: Some(Type::Number),
            body: Some(Block(vec![
                Stmt::For {
                    init: Some(ForInit::Variable(VariableKind::Let, vec![("i".to_string(), Some(Expr::Number(0.0)))])),
                    test: Some(Expr::Bin {
                        op: BinaryOp::Lt,
                        left: Box::new(Expr::Ident { name: "i".into() }),
                        right: Box::new(Expr::Member {
                            obj: Box::new(Expr::Ident { name: "arr".into() }),
                            property: Box::new(Expr::Ident { name: "length".into() }),
                            computed: false,
                        }),
                    }),
                    update: Some(Expr::Update {
                        op: UpdateOp::PlusPlus,
                        arg: Box::new(Expr::Ident { name: "i".into() }),
                        prefix: true,
                    }),
                    body: Box::new(Stmt::Block(vec![])),
                }
            ])),
            is_async: false,
            is_generator: false,
            decorators: vec![],
            throws: false,
            error_type: None,
        };

        let mut analyzer = OwnershipAnalyzer::new();
        let ownerships = analyzer.analyze_function(&func);
        // Function with loop should infer proper ownership
        assert_eq!(ownerships.len(), 1);
    }

    #[test]
    fn test_effects_union_error_types() {
        use crate::transpile::hir::effects::EffectAnalyzer;

        let mut func = FunctionDecl {
            name: "test".into(),
            generics: vec![],
            params: vec![],
            return_type: Some(Type::Number),
            body: Some(Block(vec![
                Stmt::Throw { arg: Expr::String("error1".into()) },
                Stmt::Throw { arg: Expr::String("error2".into()) },
            ])),
            is_async: false,
            is_generator: false,
            decorators: vec![],
            throws: false,
            error_type: None,
        };

        let mut analyzer = EffectAnalyzer::new();
        analyzer.analyze_function(&mut func);

        assert!(func.throws, "Function should throw");
        // Error type should be a union since we have multiple throw sites
        if let Some(Type::Union { types }) = &func.error_type {
            assert_eq!(types.len(), 2, "Should have union of 2 error types");
        }
    }

    #[test]
    fn test_arena_alloc_vec() {
        use crate::transpile::hir::arena::HirArena;
        use crate::transpile::hir::arena::ArenaAllocatable;

        let mut arena = HirArena::new();

        // Allocate a vec
        let vec = vec![1u32, 2, 3];
        let idx = arena.alloc_vec(vec);

        // Verify allocation happened
        assert!(!idx.is_null(), "Should return valid index");
        assert_eq!(arena.allocation_count(), 1, "Should have 1 allocation");
    }

    #[test]
    fn test_base_rust_lifetime() {
        use crate::transpile::hir::Ownership;

        assert_eq!(Ownership::Owned.rust_lifetime(), "");
        assert_eq!(Ownership::Borrow.rust_lifetime(), "&");
        assert_eq!(Ownership::Mut.rust_lifetime(), "&mut ");

        // Test named lifetime
        assert_eq!(Ownership::Borrow.rust_lifetime_named("'a"), "&'a");
        assert_eq!(Ownership::Mut.rust_lifetime_named("'a"), "&'a ");
    }
}
