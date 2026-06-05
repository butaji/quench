//! Jump statement tests

use super::helpers::*;

#[test]
fn parser_break_in_loop() {
    let source = wrap_in_function("for (;;) { break; }");
    let stmts = parse_function_body(&source);
    let has_break = find_stmt(&stmts, |s| matches!(s, Stmt::For { .. }))
        .map(|s| loop_body_contains(s, "break"))
        .unwrap_or(false);
    assert!(has_break, "break should parse inside for loop");
}

#[test]
fn parser_break_labeled() {
    let source = wrap_in_function("outer: for (;;) { break outer; }");
    let stmts = parse_function_body(&source);
    let has_break = find_stmt(&stmts, |s| matches!(s, Stmt::Labeled { .. }))
        .and_then(|s| {
            if let Stmt::Labeled { body, .. } = s {
                loop_body_contains(body.as_ref(), "break").then_some(true)
            } else {
                None
            }
        })
        .unwrap_or(false);
    assert!(has_break, "labeled break should parse");
}

#[test]
fn codegen_break_in_loop() {
    let source = wrap_in_function("for (;;) { break; }");
    let parser = TsParser::new();
    let result = parser.parse_source(&source).expect("parse failed");
    let func = result.items.iter().find_map(|item| {
        if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
    }).expect("no function");

    let cg = QuoteCodegen::default();
    let tokens = cg.gen_fn(func);
    let s = tokens.to_string();
    assert!(s.contains("break"), "should generate break: {}", s);
}

#[test]
fn codegen_break_labeled() {
    let source = wrap_in_function("outer: for (;;) { break outer; }");
    let parser = TsParser::new();
    let result = parser.parse_source(&source).expect("parse failed");
    let func = result.items.iter().find_map(|item| {
        if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
    }).expect("no function");

    let cg = QuoteCodegen::default();
    let tokens = cg.gen_fn(func);
    let s = tokens.to_string();
    assert!(s.contains("break"), "should generate break: {}", s);
}

#[test]
fn parser_continue_in_loop() {
    let source = wrap_in_function("for (;;) { continue; }");
    let stmts = parse_function_body(&source);
    let has_continue = find_stmt(&stmts, |s| matches!(s, Stmt::For { .. }))
        .map(|s| loop_body_contains(s, "continue"))
        .unwrap_or(false);
    assert!(has_continue, "continue should parse inside for loop");
}

#[test]
fn parser_continue_labeled() {
    let source = wrap_in_function("outer: for (;;) { continue outer; }");
    let stmts = parse_function_body(&source);
    let has_continue = find_stmt(&stmts, |s| matches!(s, Stmt::Labeled { .. }))
        .and_then(|s| {
            if let Stmt::Labeled { body, .. } = s {
                loop_body_contains(body.as_ref(), "continue").then_some(true)
            } else {
                None
            }
        })
        .unwrap_or(false);
    assert!(has_continue, "labeled continue should parse");
}

#[test]
fn codegen_continue_in_loop() {
    let source = wrap_in_function("for (;;) { continue; }");
    let parser = TsParser::new();
    let result = parser.parse_source(&source).expect("parse failed");
    let func = result.items.iter().find_map(|item| {
        if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
    }).expect("no function");

    let cg = QuoteCodegen::default();
    let tokens = cg.gen_fn(func);
    let s = tokens.to_string();
    assert!(s.contains("continue"), "should generate continue: {}", s);
}

#[test]
fn codegen_continue_labeled() {
    let source = wrap_in_function("outer: for (;;) { continue outer; }");
    let parser = TsParser::new();
    let result = parser.parse_source(&source).expect("parse failed");
    let func = result.items.iter().find_map(|item| {
        if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
    }).expect("no function");

    let cg = QuoteCodegen::default();
    let tokens = cg.gen_fn(func);
    let s = tokens.to_string();
    assert!(s.contains("continue"), "should generate continue: {}", s);
}

#[test]
fn parser_return_no_arg() {
    let source = wrap_in_function("return;");
    let stmts = parse_function_body(&source);
    let has_return = stmts.iter().any(|s| matches!(s, Stmt::Return { arg: None }));
    assert!(has_return, "return without arg should parse");
}

#[test]
fn parser_return_with_expr() {
    let source = wrap_in_function("return 1;");
    let stmts = parse_function_body(&source);
    let has_return = stmts.iter().any(|s| matches!(s, Stmt::Return { arg: Some(_) }));
    assert!(has_return, "return with expr should parse");
}

#[test]
fn parser_return_with_object() {
    let source = wrap_in_function("return { a: 1, b: 2 };");
    let stmts = parse_function_body(&source);
    let has_return = stmts.iter().any(|s| matches!(s, Stmt::Return { arg: Some(_) }));
    assert!(has_return, "return with object should parse");
}

#[test]
fn parser_return_with_array() {
    let source = wrap_in_function("return [1, 2, 3];");
    let stmts = parse_function_body(&source);
    let has_return = stmts.iter().any(|s| matches!(s, Stmt::Return { arg: Some(_) }));
    assert!(has_return, "return with array should parse");
}

#[test]
fn parser_return_with_function_call() {
    let source = wrap_in_function("return foo(1, 2);");
    let stmts = parse_function_body(&source);
    let has_return = stmts.iter().any(|s| matches!(s, Stmt::Return { arg: Some(_) }));
    assert!(has_return, "return with function call should parse");
}

#[test]
fn codegen_return_no_arg() {
    let stmt = Stmt::Return { arg: None };
    let tokens = assert_codegen_some(&stmt, "return no arg");
    let s = tokens.to_string();
    assert!(s.contains("return"), "should generate return: {}", s);
}

#[test]
fn codegen_return_with_expr() {
    let stmt = Stmt::Return { arg: Some(Expr::Number(42.0)) };
    let tokens = assert_codegen_some(&stmt, "return with expr");
    let s = tokens.to_string();
    assert!(s.contains("return"), "should generate return: {}", s);
}

#[test]
fn codegen_return_with_object() {
    let stmt = Stmt::Return {
        arg: Some(Expr::Object {
            members: vec![ObjectMemberExpr {
                prop: ObjectProp::Init {
                    key: PropKey::Str("a".into()),
                    value: Expr::Number(1.0),
                    computed: false,
                },
            }],
        }),
    };
    let tokens = assert_codegen_some(&stmt, "return with object");
    let s = tokens.to_string();
    assert!(s.contains("return"), "should generate return: {}", s);
}

#[test]
fn integration_return_in_function() {
    let source = wrap_in_function("if (x) { return 1; } return 0;");
    let parser = TsParser::new();
    let result = parser.parse_source(&source).expect("parse failed");
    let func = result.items.iter().find_map(|item| {
        if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
    }).expect("no function");

    let cg = QuoteCodegen::default();
    let tokens = cg.gen_fn(func);
    let s = tokens.to_string();
    assert!(s.contains("return"), "should generate return: {}", s);
}

#[test]
fn parser_labeled_basic() {
    assert_not_empty("label: { }", "labeled basic");
}

#[test]
fn parser_labeled_with_loop() {
    assert_not_empty("outer: while (true) { break outer; }", "labeled with loop");
}

#[test]
fn codegen_labeled_basic() {
    let stmt = parse_first_stmt("label: { }");
    let tokens = assert_codegen_some(&stmt, "labeled basic");
    let s = tokens.to_string();
    assert!(!contains_value_null(&tokens), "should not fallback to Value::Null");
}

#[test]
fn codegen_labeled_with_loop() {
    let source = wrap_in_function("outer: while (true) { break outer; }");
    let parser = TsParser::new();
    let result = parser.parse_source(&source).expect("parse failed");
    let func = result.items.iter().find_map(|item| {
        if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
    }).expect("no function");

    let cg = QuoteCodegen::default();
    let tokens = cg.gen_fn(func);
    let s = tokens.to_string();
    assert!(s.contains("while"), "should generate while: {}", s);
    assert!(s.contains("break"), "should generate break: {}", s);
}
