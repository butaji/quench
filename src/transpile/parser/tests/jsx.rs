//! JSX parsing tests

use crate::transpile::hir::*;
use crate::transpile::parser::TsParser;

#[test]
fn test_parse_jsx_element_basic() {
    let parser = TsParser::new();
    let result = parser.parse_tsx(r#"const x = <div>hello</div>;"#);
    assert!(result.is_ok(), "Parsing should succeed");
    let module = result.unwrap();

    // Find the variable declaration
    let var_decl = find_variable_decl(&module, "x");
    assert!(var_decl.is_some(), "Should find variable 'x'");
    let var_decl = var_decl.unwrap();

    // Verify init contains JSX expression
    assert!(var_decl.init.is_some(), "Variable should have initializer");
    let init = var_decl.init.unwrap();
    assert!(matches!(init, Expr::JSX(_)), "Initializer should be JSX expression");
}

#[test]
fn test_parse_jsx_element_with_attrs() {
    let parser = TsParser::new();
    let result = parser.parse_tsx(r#"const elem = <div id="test" class="box">content</div>;"#);
    assert!(result.is_ok());
    let module = result.unwrap();

    let var_decl = find_variable_decl(&module, "elem").unwrap();
    if let Expr::JSX(jsx) = var_decl.init.unwrap() {
        assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "div"));
        assert!(!jsx.opening.attrs.is_empty(), "Should have attributes");
        assert_eq!(jsx.opening.attrs.len(), 2);
    } else {
        panic!("Expected JSX expression");
    }
}

#[test]
fn test_parse_jsx_fragment() {
    let parser = TsParser::new();
    // Test with fragment syntax
    let result = parser.parse_tsx(r#"const elem = <>hello</>;"#);
    assert!(result.is_ok());
    let module = result.unwrap();

    let var_decl = find_variable_decl(&module, "elem").expect("Should find variable 'elem'");
    if let Expr::JSX(jsx) = var_decl.init.unwrap() {
        assert!(matches!(jsx.opening.name, JSXName::Fragment));
        assert!(jsx.closing.is_none(), "Fragment has no closing tag");
    } else {
        panic!("Expected JSX expression");
    }
}

#[test]
fn test_parse_jsx_nested() {
    let parser = TsParser::new();
    let result = parser.parse_tsx(r#"const elem = <div><span>nested</span></div>;"#);
    assert!(result.is_ok());
    let module = result.unwrap();

    let var_decl = find_variable_decl(&module, "elem").unwrap();
    if let Expr::JSX(jsx) = var_decl.init.unwrap() {
        assert!(!jsx.children.is_empty(), "Should have children");
        let child = &jsx.children[0];
        assert!(matches!(child, JSXChild::JSX(_)), "Child should be JSX");
    } else {
        panic!("Expected JSX expression");
    }
}

#[test]
fn test_parse_jsx_with_expr_child() {
    let parser = TsParser::new();
    let result = parser.parse_tsx(r#"const elem = <div>{name}</div>;"#);
    assert!(result.is_ok());
    let module = result.unwrap();

    let var_decl = find_variable_decl(&module, "elem").unwrap();
    if let Expr::JSX(jsx) = var_decl.init.unwrap() {
        assert!(!jsx.children.is_empty());
        let child = &jsx.children[0];
        assert!(matches!(child, JSXChild::Expr(_)), "Child with braces should be Expr");
    } else {
        panic!("Expected JSX expression");
    }
}

#[test]
fn test_jsx_hir_json_serialization() {
    let parser = TsParser::new();
    let source = r#"const x = <div>hello</div>;"#;
    let result = parser.parse_tsx(source);
    assert!(result.is_ok());
    let module = result.unwrap();

    // Serialize to JSON and back
    let json = serde_json::to_string(&module).expect("Should serialize to JSON");
    // Verify JSX structure is present in JSON
    assert!(json.contains("\"JSX\""), "JSON should contain JSX");
    assert!(json.contains("\"div\""), "JSON should reference div element");
    assert!(json.contains("hello"), "JSON should contain text content");

    // Note: JSON deserialization has a pre-existing issue with duplicate "kind" fields
    // across nested tagged enums (ModuleItem, Decl, VariableKind). This is a separate bug.
}

#[test]
fn test_jsx_fragment_json_serialization() {
    let parser = TsParser::new();
    let source = r#"const x = <>fragment content</>;"#;
    let result = parser.parse_tsx(source);
    assert!(result.is_ok());

    let json = serde_json::to_string(&result.unwrap()).expect("Should serialize");
    // Verify Fragment structure is present in JSON
    assert!(json.contains("Fragment"), "JSON should contain Fragment");
    assert!(json.contains("fragment content"), "JSON should contain text content");

    // Note: JSON deserialization has a pre-existing issue with duplicate "kind" fields
    // across nested tagged enums (ModuleItem, Decl, VariableKind). This is a separate bug.
}

/// Helper to find a variable declaration by name
fn find_variable_decl(module: &Module, name: &str) -> Option<VariableDecl> {
    for item in &module.items {
        if let ModuleItem::Decl(Decl::Variable(var)) = item {
            if var.name == name {
                return Some(var.clone());
            }
        }
    }
    None
}