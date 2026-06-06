    let result = plugin.try_codegen_jsx(&items);
    assert!(result.is_some());
    let code = normalize(&result.unwrap());
    // Should have Text::new with format! macro for the expression
    assert!(code.contains("Text::new"), "missing Text::new: {}", code);
    // Should have the identifier
    assert!(code.contains("count") || code.contains("format!"), 
            "missing count expression: {}", code);
}

#[test]
fn run_ink_dev_bordered_example() {
    // The dev path: read the .tsx, lower to JS,
    // eval through rquickjs+bridge, render to
    // string. End-to-end test using the real
    // bordered example source.
    let src = include_str!("../../../examples/ink-bordered/tui/app.tsx");
    let transformed = crate::dev_jsx::transform(src);
    // dev_eval_program works on the ORIGINAL
    // source (it parses JSX tags). The
    // transformed JS has no JSX tags, so we
    // pass the source itself.
    let program = crate::plugin::dev_eval_program_with_lowered(src, &transformed.js);
    let result = crate::plugin::run_ink_dev_with_program(&transformed.js, &program);
    assert!(result.is_ok(), "dev path failed: {:?}", result);
    let s = result.unwrap();
    assert!(s.contains("Bordered Example"), "missing title: {s}");
    assert!(
        s.contains('╭') || s.contains('╯') || s.contains('╮') || s.contains('│'),
        "missing border: {s}"
    );
}

#[test]
fn dev_render_bordered_example() {
    // ATOMIC TEST: rquickjs dev path renders the
    // bordered example through the full pipeline.
    let src = include_str!("../../../examples/ink-bordered/tui/app.tsx");
    let transformed = crate::dev_jsx::transform(src);
    let program = crate::plugin::dev_eval_program_with_lowered(src, &transformed.js);
    let result = crate::plugin::run_ink_dev_with_program(&transformed.js, &program);
    assert!(result.is_ok(), "dev render failed: {:?}", result);
    let output = result.unwrap();
    assert!(output.contains("Bordered Example"),
        "dev render output missing title: {output:?}");
}

#[test]
fn dev_render_aligned_example() {
    // ATOMIC TEST: rquickjs dev path renders the
    // aligned example correctly.
    let src = include_str!("../../../examples/ink-aligned/tui/app.tsx");
    let transformed = crate::dev_jsx::transform(src);
    let program = crate::plugin::dev_eval_program_with_lowered(src, &transformed.js);
    let result = crate::plugin::run_ink_dev_with_program(&transformed.js, &program);
    assert!(result.is_ok(), "dev render failed: {:?}", result);
    let output = result.unwrap();
    // Look for "Centered" in the output - the example has a centered title
    assert!(output.contains("Centered") || output.contains("centered"),
        "dev render output missing 'Centered': {output:?}");
}

// =============================================================================
// Variable extraction and codegen tests
// =============================================================================

/// Test that extract_var_declarations correctly extracts const count = 0
#[test]
fn test_extract_var_declarations_const_number() {
    // Simulate the HIR body structure: body is an array containing a Block
    let body = json!([
        {
            "kind": "Block",
            "stmts": [
                {
                    "kind": "Expr",
                    "expr": {
                        "Assign": {
                            "op": "Assign",
                            "left": { "Ident": { "name": "count" } },
                            "right": { "Number": 0.0 }
                        }
                    }
                }
            ]
        },
        {
            "kind": "Return",
            "arg": { "kind": "String", "0": "test" }
        }
    ]);
    
    let decls = extract_var_declarations(&body);
    assert!(!decls.is_empty(), "should extract at least one declaration");
    assert!(decls[0].contains("count"), "should extract count variable");
    assert!(decls[0].contains("0"), "should include the value 0");
}

/// Test that extract_var_declarations handles string values
#[test]
fn test_extract_var_declarations_string_value() {
    let body = json!([
        {
            "kind": "Block",
            "stmts": [
                {
                    "kind": "Expr",
                    "expr": {
                        "Assign": {
                            "op": "Assign",
                            "left": { "Ident": { "name": "name" } },
                            "right": { "String": "hello" }
                        }
                    }
                }
            ]
        }
    ]);
    
    let decls = extract_var_declarations(&body);
    assert!(!decls.is_empty(), "should extract declaration");
    assert!(decls[0].contains("name"), "should extract name variable");
    assert!(decls[0].contains("hello"), "should include string value");
}

/// Test that expr_value_to_rust handles Ident correctly
#[test]
fn test_expr_value_to_rust_ident() {
    // {"Ident": {"name": "count"}}
    let value = json!({
        "Ident": { "name": "count" }
    });
    
    let result = expr_value_to_rust(&value);
    assert!(result.is_some(), "should convert Ident to Rust");
    let rust = result.unwrap();
    // Should produce "count" as an identifier, not as a string literal
    // The format_ident! produces a proper identifier token
    assert!(rust.contains("count"), "should contain count: {rust}");
}

/// Test that expr_value_to_rust handles Number correctly
#[test]
fn test_expr_value_to_rust_number() {
    let value = json!({"Number": 42.0});
    let result = expr_value_to_rust(&value);
    assert!(result.is_some(), "should convert Number to Rust");
    let rust = result.unwrap();
    assert!(rust.contains("42"), "should contain 42: {rust}");
}

/// Test that expr_value_to_rust handles String correctly
#[test]
fn test_expr_value_to_rust_string() {
    let value = json!({"String": "test"});
    let result = expr_value_to_rust(&value);
    assert!(result.is_some(), "should convert String to Rust");
    let rust = result.unwrap();
    assert!(rust.contains("\"test\""), "should contain quoted string: {rust}");
}

/// Test that expr_value_to_rust handles Bool correctly
#[test]
fn test_expr_value_to_rust_bool() {
    let value_true = json!({"Bool": true});
    let result_true = expr_value_to_rust(&value_true);
    assert!(result_true.is_some());
    assert!(result_true.unwrap().contains("true"), "should contain true");
    
    let value_false = json!({"Bool": false});
    let result_false = expr_value_to_rust(&value_false);
    assert!(result_false.is_some());
    assert!(result_false.unwrap().contains("false"), "should contain false");
}

/// Test the full codegen with variable declarations
#[test]
fn test_codegen_with_variable_declarations() {
    let plugin = ratatui_plugin();
    let items = json!([{"Decl": {"Function": {"name": "Counter", "body": [{"kind": "Block", "stmts": [{"kind": "Expr", "expr": {"Assign": {"op": "Assign", "left": { "Ident": { "name": "count" }}, "right": { "Number": 0.0}}}}]}, {"kind": "Return", "arg": {"JSX": {"opening": { "name": { "Ident": "Box" }, "attrs": [], "self_closing": false}, "closing": { "name": { "Ident": "Box" }}, "children": [{"kind": "JSX", "opening": { "name": { "Ident": "Text" }, "attrs": [], "self_closing": false}, "closing": { "name": { "Ident": "Text" }}, "children": [{"Text": "Count: "}, {"Expr": { "Ident": { "name": "count" }}}]}}]}}}}}]);
    let result = plugin.try_codegen_jsx(&items);
    assert!(result.is_some(), "codegen should succeed");
    let code = result.unwrap();
    assert!(code.contains("let count") || code.contains("count"), "should include count variable: {code}");
    let code_normalized = code.replace(" :: ", "::").replace(" ::", "::").replace(":: ", "::");
    assert!(code_normalized.contains("Text::new"), "should include Text::new: {code}");
}

/// Test that extract_jsx_from_function_with_vars extracts both JSX and variables
#[test]
fn test_extract_jsx_from_function_with_vars() {
    let func_item = json!({"Decl": {"Function": {"name": "Counter", "body": [{"kind": "Block", "stmts": [{"kind": "Expr", "expr": {"Assign": {"op": "Assign", "left": { "Ident": { "name": "count" }}, "right": { "Number": 0.0}}}}]}, {"kind": "Return", "arg": {"JSX": {"opening": { "name": { "Ident": "Box" }, "attrs": [], "self_closing": false}, "closing": { "name": { "Ident": "Box" }}, "children": []}}}}]}}}}});
    let result = extract_jsx_from_function_with_vars(&func_item);
    assert!(result.is_some(), "should extract JSX and variables");
    let (jsx, var_decls) = result.unwrap();
    assert!(jsx.get("opening").is_some(), "should extract JSX");
    assert!(!var_decls.is_empty(), "should extract var declarations");
