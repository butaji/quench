//! Loop tests

use super::helpers::*;

// C-style for

#[test]
fn parser_for_basic() {
    assert_not_empty("for (let i = 0; i < 10; i++) { }", "for basic");
}

#[test]
fn parser_for_no_init() {
    assert_not_empty("for (; i < 10; i++) { }", "for no init");
}

#[test]
fn parser_for_no_test() {
    assert_not_empty("for (let i = 0; ; i++) { }", "for no test");
}

#[test]
fn parser_for_no_update() {
    assert_not_empty("for (let i = 0; i < 10;) { }", "for no update");
}

#[test]
fn parser_for_empty_all() {
    assert_not_empty("for (;;) { }", "for empty all");
}

#[test]
fn codegen_for_basic() {
    let stmt = parse_first_stmt("for (let i = 0; i < 10; i++) { }");
    let tokens = assert_codegen_some(&stmt, "for basic");
    let s = tokens.to_string();
    assert!(s.contains("for"), "should contain for: {}", s);
    assert!(s.contains(";"), "should have semicolons: {}", s);
}

#[test]
fn codegen_for_no_init() {
    let stmt = parse_first_stmt("for (; i < 10; i++) { }");
    let tokens = assert_codegen_some(&stmt, "for no init");
    let s = tokens.to_string();
    assert!(s.contains("for"), "should contain for: {}", s);
}

#[test]
fn codegen_for_empty_all() {
    let stmt = parse_first_stmt("for (;;) { }");
    let tokens = assert_codegen_some(&stmt, "for empty all");
    let s = tokens.to_string();
    assert!(s.contains("for"), "should contain for: {}", s);
}

// For...of

#[test]
fn parser_for_of() {
    assert_not_empty("for (const x of arr) { }", "for-of");
}

#[test]
fn parser_for_of_let() {
    assert_not_empty("for (let x of arr) { }", "for-of let");
}

#[test]
fn parser_for_of_with_body() {
    assert_not_empty("for (const x of arr) { const y = x; }", "for-of with body");
}

#[test]
fn codegen_for_of() {
    let stmt = parse_first_stmt("for (const x of arr) { }");
    let tokens = assert_codegen_some(&stmt, "for-of");
    let s = tokens.to_string();
    assert!(s.contains("for") && s.contains("in"), "for-of should become for...in: {}", s);
}

#[test]
fn codegen_for_of_let() {
    let stmt = parse_first_stmt("for (let x of arr) { }");
    let tokens = assert_codegen_some(&stmt, "for-of let");
    let s = tokens.to_string();
    assert!(s.contains("for") && s.contains("in"), "for-of should become for...in: {}", s);
}

// For...in

#[test]
fn parser_for_in() {
    assert_not_empty("for (const k in obj) { }", "for-in");
}

#[test]
fn parser_for_in_let() {
    assert_not_empty("for (let k in obj) { }", "for-in let");
}

#[test]
fn codegen_for_in() {
    let stmt = parse_first_stmt("for (const k in obj) { }");
    let tokens = assert_codegen_some(&stmt, "for-in");
    let s = tokens.to_string();
    assert!(s.contains("for") && s.contains("in"), "for-in should contain for...in: {}", s);
}

#[test]
fn codegen_for_in_let() {
    let stmt = parse_first_stmt("for (let k in obj) { }");
    let tokens = assert_codegen_some(&stmt, "for-in let");
    let s = tokens.to_string();
    assert!(s.contains("for") && s.contains("in"), "for-in should contain for...in: {}", s);
}

// While

#[test]
fn parser_while_basic() {
    assert_not_empty("while (x) { }", "while basic");
}

#[test]
fn parser_while_with_body() {
    assert_not_empty("while (x) { const y = 1; }", "while with body");
}

#[test]
fn codegen_while_basic() {
    let stmt = parse_first_stmt("while (x) { }");
    let tokens = assert_codegen_some(&stmt, "while basic");
    let s = tokens.to_string();
    assert!(s.contains("while"), "should contain while: {}", s);
}

#[test]
fn codegen_while_with_body() {
    let stmt = parse_first_stmt("while (x) { const y = 1; }");
    let tokens = assert_codegen_some(&stmt, "while with body");
    let s = tokens.to_string();
    assert!(s.contains("while"), "should contain while: {}", s);
}

// Do...while

#[test]
fn parser_do_while() {
    assert_not_empty("do { } while (x);", "do-while");
}

#[test]
fn parser_do_while_with_body() {
    assert_not_empty("do { const y = 1; } while (x);", "do-while with body");
}

#[test]
fn codegen_do_while() {
    let stmt = parse_first_stmt("do { } while (x);");
    let tokens = assert_codegen_some(&stmt, "do-while");
    let s = tokens.to_string();
    assert!(s.contains("loop") && s.contains("break"), "do-while should become loop with break: {}", s);
}

#[test]
fn codegen_do_while_with_body() {
    let stmt = parse_first_stmt("do { const y = 1; } while (x);");
    let tokens = assert_codegen_some(&stmt, "do-while with body");
    let s = tokens.to_string();
    assert!(s.contains("loop") && s.contains("break"), "do-while should become loop with break: {}", s);
}

// Integration

#[test]
fn integration_for_with_break() {
    let source = wrap_in_function("for (let i = 0; i < 10; i++) { if (i > 5) { break; } }");
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

#[test]
fn integration_for_of_with_continue() {
    let source = wrap_in_function("for (const x of arr) { if (x === 0) { continue; } }");
    let parser = TsParser::new();
    let result = parser.parse_source(&source).expect("parse failed");
    let func = result.items.iter().find_map(|item| {
        if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
    }).expect("no function");

    let cg = QuoteCodegen::default();
    let tokens = cg.gen_fn(func);
    let s = tokens.to_string();
    assert!(s.contains("for") && s.contains("in"), "should generate for...in: {}", s);
    assert!(s.contains("continue"), "should generate continue: {}", s);
}
