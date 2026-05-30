#[cfg(test)]
mod parser_tests {
    use crate::transpile::hir::*;
    use crate::transpile::parser::TsParser;

    #[test]
    fn test_parse_import() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"import { useState } from "preact/hooks";"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_type_alias() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"type Props = { count: number; };"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_interface() {
        let parser = TsParser::new();
        let result = parser.parse_source(
            r#"interface CounterProps { initial?: number; step?: number; label?: string; }"#,
        );
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_function() {
        let parser = TsParser::new();
        let result =
            parser.parse_source(r#"function add(a: number, b: number): number { return a + b; }"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_async_function() {
        let parser = TsParser::new();
        let result = parser.parse_source(
            r#"async function fetchData(url: string): Promise<Response> { return fetch(url); }"#,
        );
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_jsx_element() {
        let parser = TsParser::new();
        let result = parser.parse_tsx(r#"const elem = <div>Hello</div>;"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_jsx_fragment() {
        let parser = TsParser::new();
        let result = parser.parse_tsx(r#"const elem = <>Hello <span>world</span></>;"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_jsx_fragment_empty() {
        let parser = TsParser::new();
        let result = parser.parse_tsx(r#"const elem = <></>;"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_jsx_component() {
        let parser = TsParser::new();
        let result = parser.parse_tsx(r#"const comp = <Counter initial={0} step={1} />;"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_template_literal() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"const msg = `Hello ${name}`;"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_destructuring_object() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"const { name, age } = person;"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_destructuring_array() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"const [first, ...rest] = items;"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_conditional() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"const result = count > 0 ? "positive" : "negative";"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_logical_operators() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"const a = x && y || z;"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_parse_use_state() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"const [count, setCount] = useState(0);"#);
        assert!(result.is_ok());
    }
    #[test]
    fn test_export_default_function_is_decl_not_stmt() {
        let parser = TsParser::new();
        let source = "export default function Hello() { return 42; }";
        let result = parser.parse_source(source);
        assert!(result.is_ok(), "Parsing should succeed: {:?}", result.err());
        let module = result.unwrap();

        // Find the function declaration
        let func_item = module.items.iter().find_map(|item| {
            if let ModuleItem::Decl(Decl::Function(f)) = item {
                Some(f.clone())
            } else {
                None
            }
        });

        assert!(
            func_item.is_some(),
            "Expected to find ModuleItem::Decl(Decl::Function(...)), but got items: {:#?}",
            module.items
        );

        let func = func_item.unwrap();
        assert_eq!(func.name, "Hello", "Function should be named 'Hello'");
    }
    #[test]
    fn test_export_default_function_json_serialization() {
        let parser = TsParser::new();
        let source = "export default function Hello() { return 42; }";
        let result = parser.parse_source(source).expect("Parsing should succeed");
        let module = result;

        // Serialize to JSON
        let json = serde_json::to_string(&module).expect("Should serialize to JSON");

        // Print full JSON for comparison
        println!("\n=== Full JSON (non-JSX) ===\n{}\n===\n", serde_json::to_string_pretty(&json).unwrap());

        // Verify the JSON contains Decl::Function structure
        // Should be: {"kind":"Decl","Function":{"name":"Hello",...}}
        assert!(
            json.contains("\"kind\":\"Decl\""),
            "JSON should contain '{{\"kind\":\"Decl\"}}' but got: {}",
            json
        );
        assert!(
            json.contains("\"Function\""),
            "JSON should contain '{{\"Function\"}}' but got: {}",
            json
        );

        // Verify the JSON does NOT contain Stmt::Empty
        // This would indicate the bug where export default becomes Empty
        assert!(
            !json.contains("\"kind\":\"Stmt\"") || !json.contains("\"Empty\""),
            "JSON should NOT contain Stmt::Empty, but got: {}",
            json
        );
    }
    #[test]
    fn test_export_default_anonymous_function() {
        let parser = TsParser::new();
        let source = "export default function() { return 42; }";
        let result = parser.parse_source(source);
        assert!(result.is_ok(), "Parsing should succeed");
        let module = result.unwrap();

        // Find the function declaration
        let func_item = module.items.iter().find_map(|item| {
            if let ModuleItem::Decl(Decl::Function(f)) = item {
                Some(f.clone())
            } else {
                None
            }
        });

        assert!(
            func_item.is_some(),
            "Expected to find ModuleItem::Decl(Decl::Function(...)), got: {:#?}",
            module.items
        );
    }
    #[test]
    fn test_export_default_function_with_params() {
        let parser = TsParser::new();
        let source = "export default function add(a: number, b: number): number { return a + b; }";
        let result = parser.parse_source(source).expect("Parsing should succeed");
        let module = result;

        // Find the function declaration
        let func = module.items.iter().find_map(|item| {
            if let ModuleItem::Decl(Decl::Function(f)) = item {
                Some(f)
            } else {
                None
            }
        });

        assert!(func.is_some(), "Should find function declaration");
        let func = func.unwrap();
        assert_eq!(func.name, "add");
    }
    #[test]
    fn test_parse_errors_aggregated() {
        let parser = TsParser::new();
        // This source has multiple parse errors
        let source = "const x = <<<; export default function() {};";
        let result = parser.parse_source(source);
        assert!(result.is_err(), "Parsing should fail with errors");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Parse errors:") || err.to_lowercase().contains("parse error"),
            "Error should mention parse errors, got: {}",
            err
        );
    }

    #[test]
    fn test_export_default_function_with_jsx_json() {
        let source = r#"export default function Hello() { return <div>Hello</div>; }"#;
        let module = TsParser::new().parse_tsx(source).expect("Parsing should succeed");

        // Print debug info about module structure
        println!("\n=== Module items debug ===");
        for (i, item) in module.items.iter().enumerate() {
            println!("Item {}: {:?}", i, item);
        }
        println!("===\n");

        // Serialize to JSON
        let json = serde_json::to_string(&module).expect("Should serialize to JSON");
        let json_value: serde_json::Value = serde_json::from_str(&json).expect("Should parse JSON");
        let items = json_value.get("items").expect("Should have items");

        println!("\n=== HIR JSON items array ===\n{}\n===\n", serde_json::to_string_pretty(items).unwrap());

        let items_str = serde_json::to_string(items).unwrap();
        // The SERIALIZED JSON shows "kind":"Function" due to serde nested tag issue
        // But the actual Rust struct is ModuleItem::Decl(Decl::Function(...))
        // We verify by checking the FunctionDecl fields exist
        let has_function_name = items_str.contains("\"name\":\"Hello\"");
        let has_function_kind = items_str.contains("\"kind\":\"Function\"");
        let has_stmt_empty = items_str.contains("\"kind\":\"Stmt\"") && items_str.contains("\"Empty\"");

        println!("Has function name 'Hello': {}", has_function_name);
        println!("Has function kind: {}", has_function_kind);
        println!("Has Stmt::Empty: {}", has_stmt_empty);

        // The key verification: NOT Stmt::Empty
        if has_stmt_empty {
            panic!("BUG: items contain Stmt::Empty: {}", items_str);
        }

        // Function should be properly parsed (even if serde serialization is weird)
        assert!(has_function_name, "Function should have name 'Hello'");
        assert!(has_function_kind, "Function should have kind 'Function'");
    }
}
