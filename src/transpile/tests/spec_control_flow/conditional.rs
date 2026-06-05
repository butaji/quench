//! Conditional tests

use super::helpers::*;

#[test]
fn parser_if_basic() {
    assert_not_empty("if (x) { }", "if basic");
}

#[test]
fn parser_if_with_block() {
    assert_not_empty("if (x) { const y = 1; }", "if with block");
}

#[test]
fn parser_if_else() {
    assert_not_empty("if (x) { } else { }", "if-else");
}

#[test]
fn parser_if_else_if() {
    assert_not_empty("if (x) { } else if (y) { }", "if-else if");
}

#[test]
fn parser_if_else_if_else() {
    assert_not_empty("if (x) { } else if (y) { } else { }", "if-else if-else");
}

#[test]
fn parser_if_nested() {
    assert_not_empty("if (x) { if (y) { } }", "nested if");
}

#[test]
fn codegen_if_basic() {
    let stmt = parse_first_stmt("if (x) { }");
    let tokens = assert_codegen_some(&stmt, "if basic");
    let s = tokens.to_string();
    assert!(s.contains("if"), "should contain if: {}", s);
    assert!(!contains_value_null(&tokens), "should not fallback to Value::Null");
}

#[test]
fn codegen_if_with_expr() {
    let stmt = parse_first_stmt("if (x > 0) { const y = 1; }");
    let tokens = assert_codegen_some(&stmt, "if with expr");
    let s = tokens.to_string();
    assert!(s.contains("if"), "should contain if: {}", s);
    assert!(!contains_value_null(&tokens), "should not fallback to Value::Null");
}

#[test]
fn codegen_if_else() {
    let stmt = parse_first_stmt("if (x) { const a = 1; } else { const b = 2; }");
    let tokens = assert_codegen_some(&stmt, "if-else");
    let s = tokens.to_string();
    assert!(s.contains("if") && s.contains("else"), "should contain if-else: {}", s);
}

#[test]
fn codegen_if_else_if_else() {
    let stmt = parse_first_stmt("if (x) { } else if (y) { } else { }");
    let tokens = assert_codegen_some(&stmt, "if-else if-else");
    let s = tokens.to_string();
    assert!(s.contains("else"), "should contain else: {}", s);
}

#[test]
fn integration_if_else_return() {
    let source = wrap_in_function("if (x) { return 1; } else { return 2; }");
    let parser = TsParser::new();
    let result = parser.parse_source(&source).expect("parse failed");
    let func = result.items.iter().find_map(|item| {
        if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
    }).expect("no function");

    let cg = QuoteCodegen::default();
    let tokens = cg.gen_fn(func);
    let s = tokens.to_string();
    assert!(s.contains("if"), "should generate if: {}", s);
    assert!(s.contains("return"), "should generate return: {}", s);
}
