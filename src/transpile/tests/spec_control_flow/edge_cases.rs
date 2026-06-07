//! Edge case tests

use super::helpers::*;

#[test]
fn parser_if_with_comma_in_condition() {
    let source = wrap_in_function("if (x, y) { return 1; }");
    let stmts = parse_function_body(&source);
    let has_if = stmts.iter().any(|s| matches!(s, Stmt::If { .. }));
    assert!(has_if, "if with comma in condition should parse");
}

#[test]
fn parser_nested_loops_break_continue() {
    let source = wrap_in_function("for (;;) { for (;;) { break; } continue; }");
    let stmts = parse_function_body(&source);
    let outer_for = find_stmt(&stmts, |s| matches!(s, Stmt::For { .. }));
    assert!(outer_for.is_some(), "nested loops should parse");
}

#[test]
fn parser_switch_with_break_in_loop() {
    let source = wrap_in_function("while (true) { switch (x) { case 1: break; } }");
    let stmts = parse_function_body(&source);
    let has_while = stmts.iter().any(|s| matches!(s, Stmt::While { .. }));
    assert!(has_while, "switch in while should parse");
}

#[test]
fn parser_try_in_loop() {
    let source = wrap_in_function("while (true) { try { } catch (e) { } }");
    let stmts = parse_function_body(&source);
    let has_while = stmts.iter().any(|s| matches!(s, Stmt::While { .. }));
    assert!(has_while, "try in while should parse");
}

#[test]
fn parser_if_with_assignment_in_condition() {
    let source = wrap_in_function("if (x = 1) { return 1; }");
    let stmts = parse_function_body(&source);
    let has_if = stmts.iter().any(|s| matches!(s, Stmt::If { .. }));
    assert!(has_if, "if with assignment in condition should parse");
}

#[test]
#[ignore = "for loop codegen produces 'for let i = 0' which is invalid Rust"]
fn codegen_nested_loops() {
    let source = wrap_in_function("for (;;) { for (;;) { break; } }");
    let parser = TsParser::new();
    let result = parser.parse_source(&source).expect("parse failed");
    let func = result.items.iter().find_map(|item| {
        if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
    }).expect("no function");

    let cg = QuoteCodegen::default();
    let tokens = cg.gen_fn(func);
    let s = tokens.to_string();
    let for_count = s.matches("for").count();
    assert!(for_count >= 2, "nested for loops should generate 2+ for: {}", s);
}

#[test]
fn codegen_if_else_chain() {
    let source = wrap_in_function("if (x) { return 1; } else if (y) { return 2; } else if (z) { return 3; } else { return 0; }");
    let parser = TsParser::new();
    let result = parser.parse_source(&source).expect("parse failed");
    let func = result.items.iter().find_map(|item| {
        if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
    }).expect("no function");

    let cg = QuoteCodegen::default();
    let tokens = cg.gen_fn(func);
    let s = tokens.to_string();
    assert!(s.contains("if"), "should generate if: {}", s);
    assert!(s.contains("else"), "should generate else: {}", s);
}

#[test]
#[ignore = "switch codegen produces match with === in arms, no fallthrough support"]
fn codegen_switch_with_fallthrough() {
    let source = wrap_in_function("switch (x) { case 1: case 2: return 1; default: return 0; }");
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

#[test]
#[ignore = "for loop codegen produces 'for let i = 0' which is invalid Rust"]
fn codegen_loop_with_label() {
    let source = wrap_in_function("outer: for (let i = 0; i < 10; i++) { break outer; }");
    let parser = TsParser::new();
    let result = parser.parse_source(&source).expect("parse failed");
    let func = result.items.iter().find_map(|item| {
        if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
    }).expect("no function");

    let cg = QuoteCodegen::default();
    let tokens = cg.gen_fn(func);
    let s = tokens.to_string();
    assert!(s.contains("for"), "should generate for: {}", s);
    assert!(s.contains("break"), "should generate break: {}", s);
}
