#[cfg(test)]
mod parser_tests {
    use crate::transpile::parser::TsParser;
    use crate::transpile::hir::*;
    
    #[test]
    fn test_parse_import() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"import { useState } from "preact/hooks";"#);
        assert!(result.is_ok());
        let module = result.unwrap();
        let import = match &module.items[0] {
            ModuleItem::Import(i) => i,
            _ => panic!("Expected import"),
        };
        assert_eq!(import.source, "preact/hooks");
        assert!(import.specifiers.iter().any(|s| matches!(s, ImportSpecifier::Named { name, .. } if name == "useState")));
    }
    
    #[test]
    fn test_parse_type_alias() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"type Props = { count: number; };"#);
        assert!(result.is_ok());
        let module = result.unwrap();
        let decl = match &module.items[0] {
            ModuleItem::Decl(Decl::Type(t)) => t,
            _ => panic!("Expected type declaration"),
        };
        assert_eq!(decl.name, "Props");
    }
    
    #[test]
    fn test_parse_interface() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"interface CounterProps { initial?: number; step?: number; label?: string; }"#);
        assert!(result.is_ok());
        let module = result.unwrap();
        let decl = match &module.items[0] {
            ModuleItem::Decl(Decl::Type(t)) => t,
            _ => panic!("Expected type declaration"),
        };
        assert_eq!(decl.name, "CounterProps");
        if let Type::Object { members } = &decl.type_ { assert_eq!(members.len(), 3); } else { panic!("Expected Object type"); }
    }
    
    #[test]
    fn test_parse_function() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"function add(a: number, b: number): number { return a + b; }"#);
        assert!(result.is_ok());
        let module = result.unwrap();
        let decl = match &module.items[0] {
            ModuleItem::Decl(Decl::Function(f)) => f,
            _ => panic!("Expected function declaration"),
        };
        assert_eq!(decl.name, "add");
        assert_eq!(decl.params.len(), 2);
        assert!(decl.return_type.is_some());
    }
    
    #[test]
    fn test_parse_async_function() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"async function fetchData(url: string): Promise<Response> { return fetch(url); }"#);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());
        let module = result.unwrap();
        let decl = match &module.items[0] {
            ModuleItem::Decl(Decl::Function(f)) => f,
            _ => panic!("Expected function declaration"),
        };
        assert!(decl.is_async);
    }
    
    #[test]
    fn test_parse_jsx_element() {
        let parser = TsParser::new();
        let result = parser.parse_tsx(r#"const elem = <div>Hello</div>;"#);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());
        let module = result.unwrap();
        let expr = match &module.items[0] {
            ModuleItem::Decl(Decl::Variable(v)) => v.init.as_ref().unwrap(),
            _ => panic!("Expected variable declaration"),
        };
        match expr {
            Expr::JSX(jsx_expr) => {
                match &jsx_expr.opening.name {
                    JSXName::Ident(name) => assert_eq!(name, "div"),
                    _ => panic!("Expected div element"),
                }
                assert_eq!(jsx_expr.children.len(), 1);
            }
            _ => panic!("Expected JSX expression"),
        }
    }
    
    #[test]
    fn test_parse_jsx_fragment() {
        let parser = TsParser::new();
        let result = parser.parse_tsx(r#"const elem = <>Hello <span>world</span></>;"#);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());
        let module = result.unwrap();
        let expr = match &module.items[0] {
            ModuleItem::Decl(Decl::Variable(v)) => v.init.as_ref().unwrap(),
            _ => panic!("Expected variable declaration"),
        };
        match expr {
            Expr::JSX(jsx_expr) => {
                assert!(matches!(jsx_expr.opening.name, JSXName::Fragment), "Expected fragment name");
                assert_eq!(jsx_expr.children.len(), 2);
                assert!(jsx_expr.children.iter().any(|c| matches!(c, JSXChild::Text(_))), "Expected text child");
            }
            _ => panic!("Expected JSX expression"),
        }
    }

    #[test]
    fn test_parse_jsx_fragment_empty() {
        let parser = TsParser::new();
        let result = parser.parse_tsx(r#"const elem = <></>;"#);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());
        let module = result.unwrap();
        let expr = match &module.items[0] {
            ModuleItem::Decl(Decl::Variable(v)) => v.init.as_ref().unwrap(),
            _ => panic!("Expected variable declaration"),
        };
        match expr {
            Expr::JSX(jsx_expr) => {
                assert!(matches!(jsx_expr.opening.name, JSXName::Fragment));
                assert!(jsx_expr.children.is_empty());
            }
            _ => panic!("Expected JSX expression"),
        }
    }

    #[test]
    fn test_parse_jsx_component() {
        let parser = TsParser::new();
        let source = r#"const comp = <Counter initial={0} step={1} />;"#;
        let result = parser.parse_tsx(source);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());
        let module = result.unwrap();
        let expr = match &module.items[0] {
            ModuleItem::Decl(Decl::Variable(v)) => v.init.as_ref().unwrap(),
            _ => panic!("Expected variable declaration"),
        };
        match expr {
            Expr::JSX(jsx_expr) => {
                match &jsx_expr.opening.name {
                    JSXName::Ident(name) => assert_eq!(name, "Counter"),
                    _ => panic!("Expected Counter component"),
                }
                assert!(jsx_expr.opening.self_closing);
                assert_eq!(jsx_expr.opening.attrs.len(), 2);
            }
            _ => panic!("Expected JSX expression"),
        }
    }
    
    #[test]
    fn test_parse_template_literal() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"const msg = `Hello ${name}`;"#);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());
        let module = result.unwrap();
        match &module.items[0] {
            ModuleItem::Decl(Decl::Variable(v)) => {
                assert!(matches!(v.init.as_ref().unwrap(), Expr::Template { .. }));
            }
            _ => panic!("Expected variable declaration"),
        }
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
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());
    }
    
    #[test]
    fn test_parse_use_state() {
        let parser = TsParser::new();
        let result = parser.parse_source(r#"const [count, setCount] = useState(0);"#);
        assert!(result.is_ok());
    }
}
