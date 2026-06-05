//! Switch tests

use super::helpers::*;

#[test]
fn parser_switch_basic() {
    assert_not_empty("switch (x) { case 1: break; }", "switch basic");
}

#[test]
fn parser_switch_with_default() {
    assert_not_empty("switch (x) { case 1: break; default: break; }", "switch with default");
}

#[test]
fn parser_switch_multiple_cases() {
    assert_not_empty("switch (x) { case 1: case 2: break; default: break; }", "switch multiple cases");
}

#[test]
fn parser_switch_with_return() {
    let source = wrap_in_function("switch (x) { case 1: return 1; case 2: return 2; }");
    let stmts = parse_function_body(&source);
    let has_switch = stmts.iter().any(|s| matches!(s, Stmt::Switch { .. }));
    assert!(has_switch, "switch with return should parse");
}

#[test]
fn parser_switch_fallthrough() {
    assert_not_empty("switch (x) { case 1: const a = 1; case 2: const b = 2; break; }", "switch fallthrough");
}

#[test]
fn codegen_switch_basic() {
    let stmt = parse_first_stmt("switch (x) { case 1: break; }");
    let tokens = assert_codegen_some(&stmt, "switch basic");
    let s = tokens.to_string();
    assert!(s.contains("match"), "switch should become match: {}", s);
}

#[test]
fn codegen_switch_with_default() {
    let stmt = parse_first_stmt("switch (x) { case 1: break; default: break; }");
    let tokens = assert_codegen_some(&stmt, "switch with default");
    let s = tokens.to_string();
    assert!(s.contains("match"), "switch should become match: {}", s);
    assert!(s.contains("_") || s.contains("match"), "default should become _: {}", s);
}

#[test]
fn codegen_switch_multiple_cases() {
    let stmt = parse_first_stmt("switch (x) { case 1: case 2: break; }");
    let tokens = assert_codegen_some(&stmt, "switch multiple cases");
    let s = tokens.to_string();
    assert!(s.contains("match"), "switch should become match: {}", s);
}

#[test]
fn integration_switch_basic() {
    let source = wrap_in_function("switch (x) { case 1: return 1; default: return 0; }");
    let parser = TsParser::new();
    let result = parser.parse_source(&source).expect("parse failed");
    let func = result.items.iter().find_map(|item| {
        if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
    }).expect("no function");

    let cg = QuoteCodegen::default();
    let tokens = cg.gen_fn(func);
    let s = tokens.to_string();
    assert!(s.contains("match"), "should generate match: {}", s);
}
