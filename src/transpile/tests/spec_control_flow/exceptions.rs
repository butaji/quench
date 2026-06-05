//! Exception handling tests

use super::helpers::*;

#[test]
fn parser_try_catch() {
    assert_not_empty("try { } catch (e) { }", "try-catch");
}

#[test]
fn parser_try_catch_with_body() {
    assert_not_empty("try { const x = 1; } catch (e) { const y = 2; }", "try-catch with body");
}

#[test]
fn parser_try_catch_finally() {
    assert_not_empty("try { } catch (e) { } finally { }", "try-catch-finally");
}

#[test]
fn parser_try_finally() {
    assert_not_empty("try { } finally { }", "try-finally");
}

#[test]
fn parser_try_with_throw() {
    assert_not_empty("try { throw new Error(); } catch (e) { }", "try-throw-catch");
}

#[test]
fn codegen_try_catch() {
    let stmt = parse_first_stmt("try { const x = 1; } catch (e) { const y = 2; }");
    let tokens = assert_codegen_some(&stmt, "try-catch");
    let s = tokens.to_string();
    assert!(s.contains("catch_unwind") || s.contains("match"), "try-catch should use catch_unwind: {}", s);
}

#[test]
fn codegen_try_catch_finally() {
    let stmt = parse_first_stmt("try { } catch (e) { } finally { }");
    let tokens = assert_codegen_some(&stmt, "try-catch-finally");
    let s = tokens.to_string();
    assert!(s.contains("catch_unwind") || s.contains("match"), "try-catch-finally should use catch_unwind: {}", s);
    assert!(!s.is_empty(), "should generate finally code");
}

#[test]
fn codegen_try_finally() {
    let stmt = parse_first_stmt("try { } finally { }");
    let tokens = assert_codegen_some(&stmt, "try-finally");
    let s = tokens.to_string();
    assert!(!s.is_empty(), "should generate try-finally code");
}

#[test]
fn parser_throw_new_error() {
    assert_not_empty("throw new Error();", "throw new Error");
}

#[test]
fn parser_throw_new_error_with_msg() {
    assert_not_empty(r#"throw new Error("msg");"#, "throw new Error with msg");
}

#[test]
fn parser_throw_expr() {
    assert_not_empty("throw err;", "throw expr");
}

#[test]
fn parser_throw_string() {
    assert_not_empty(r#"throw "error string";"#, "throw string");
}

#[test]
fn parser_throw_object() {
    assert_not_empty("throw { code: 404, message: \"not found\" };", "throw object");
}

#[test]
fn codegen_throw_new_error() {
    let stmt = parse_first_stmt(r#"throw new Error("msg");"#);
    let tokens = assert_codegen_some(&stmt, "throw new Error");
    let s = tokens.to_string();
    assert!(!contains_value_null(&tokens), "throw should not fallback to Value::Null");
}

#[test]
fn codegen_throw_expr() {
    let stmt = parse_first_stmt("throw err;");
    let tokens = assert_codegen_some(&stmt, "throw expr");
    let s = tokens.to_string();
    assert!(!contains_value_null(&tokens), "throw should not fallback to Value::Null");
}

#[test]
fn codegen_throw_string() {
    let stmt = parse_first_stmt(r#"throw "error";"#);
    let tokens = assert_codegen_some(&stmt, "throw string");
    let s = tokens.to_string();
    assert!(!contains_value_null(&tokens), "throw should not fallback to Value::Null");
}

#[test]
fn integration_try_catch_return() {
    let source = wrap_in_function("try { return 1; } catch (e) { return 2; }");
    let parser = TsParser::new();
    let result = parser.parse_source(&source).expect("parse failed");
    let func = result.items.iter().find_map(|item| {
        if let ModuleItem::Decl(Decl::Function(f)) = item { Some(f) } else { None }
    }).expect("no function");

    let cg = QuoteCodegen::default();
    let tokens = cg.gen_fn(func);
    let s = tokens.to_string();
    assert!(s.contains("catch_unwind") || s.contains("match"), "should use catch_unwind: {}", s);
}

#[test]
fn integration_try_catch_finally_return() {
    let source = wrap_in_function("try { return 1; } catch (e) { return 2; } finally { }");
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
fn integration_try_throw_catch_return() {
    let source = wrap_in_function("try { throw new Error(); } catch (e) { return 1; }");
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
fn integration_try_finally_with_return() {
    let source = wrap_in_function("try { return 1; } finally { }");
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
fn parser_throw_type_error() {
    assert_not_empty("throw new TypeError();", "throw TypeError");
}

#[test]
fn parser_throw_range_error() {
    assert_not_empty("throw new RangeError();", "throw RangeError");
}

#[test]
fn parser_throw_with_template() {
    assert_not_empty(r#"throw new Error(`Error: ${code}`);"#, "throw with template");
}
