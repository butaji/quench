    assert!(!var_decls.is_empty(), "should extract var declarations");
    assert!(var_decls[0].contains("count"), "should include count in declarations");
}
/// Test that codegen handles nested JSX correctly
#[test]
fn test_codegen_nested_jsx() {
    use crate::codegen::jsx::{extract_jsx_from_function_with_vars, generate_widget_for_jsx};
    let func_item = json!({"Decl": {"Function": {"name": "Nested", "body": [{"kind": "Return", "arg": {"JSX": {"opening": { "name": { "Ident": "Box" }, "attrs": [], "self_closing": false}, "closing": { "name": { "Ident": "Box" }}, "children": [{"kind": "JSX", "opening": { "name": { "Ident": "Box" }, "attrs": [], "self_closing": false}, "closing": { "name": { "Ident": "Box" }}, "children": [{"kind": "Text", "text": "Inner"}]}]}}}}]}}}}});
    let result = extract_jsx_from_function_with_vars(&func_item);
    assert!(result.is_some(), "should extract JSX");
    let (jsx, _) = result.unwrap();
    let widget = generate_widget_for_jsx(jsx);
    assert!(widget.is_some(), "should generate widget for nested JSX");
}

/// Test that codegen handles Text with expressions
#[test]
fn test_codegen_text_with_expr() {
    use crate::codegen::jsx::{extract_jsx_from_function_with_vars, generate_widget_for_jsx};

    let func_item = json!({
        "Decl": {
            "Function": {
                "name": "WithExpr",
                "body": [
                    {
                        "kind": "Return",
                        "arg": {
                            "JSX": {
                                "opening": { "name": { "Ident": "Text" }, "attrs": [], "self_closing": false },
                                "closing": { "name": { "Ident": "Text" } },
                                "children": [
                                    { "Text": "Count: " },
                                    { "Expr": { "Ident": { "name": "count" } } }
                                ]
                            }
                        }
                    }
                ]
            }
        }
    });

    let result = extract_jsx_from_function_with_vars(&func_item);
    assert!(result.is_some(), "should extract JSX");
    let (jsx, _) = result.unwrap();

    let widget = generate_widget_for_jsx(jsx);
    assert!(widget.is_some(), "should generate widget for Text with expressions");
}

/// Test that codegen handles Box with multiple children
#[test]
fn test_codegen_box_multiple_children() {
    use crate::codegen::jsx::{extract_jsx_from_function_with_vars, generate_widget_for_jsx};

    let func_item = json!({
        "Decl": {
            "Function": {
                "name": "MultiChild",
                "body": [
                    {
                        "kind": "Return",
                        "arg": {
                            "JSX": {
                                "opening": { "name": { "Ident": "Box" }, "attrs": [], "self_closing": false },
                                "closing": { "name": { "Ident": "Box" } },
                                "children": [
                                    { "kind": "Text", "text": "First" },
                                    { "kind": "Text", "text": "Second" },
                                    { "kind": "Text", "text": "Third" }
                                ]
                            }
                        }
                    }
                ]
            }
        }
    });

    let result = extract_jsx_from_function_with_vars(&func_item);
    assert!(result.is_some(), "should extract JSX");
    let (jsx, _) = result.unwrap();

    let widget = generate_widget_for_jsx(jsx);
    assert!(widget.is_some(), "should generate widget with multiple children");
}

/// Test that codegen handles FlexDirection prop
#[test]
fn test_codegen_flex_direction() {
    let plugin = RatatuiPlugin;

    let box_jsx = serde_json::json!({
        "kind": "JSX",
        "opening": {
            "name": { "Ident": "Box" },
            "attrs": [
                {
                    "Attr": {
                        "name": "flexDirection",
                        "value": { "String": "column" }
                    }
                }
            ],
            "self_closing": false
        },
        "children": []
    });

    let items = items_with_fn("FlexDirTest", box_jsx);
    let result = plugin.try_codegen_jsx(&items);
    assert!(result.is_some(), "codegen returned None");
    let code = normalize(&result.unwrap());
    assert!(code.contains("flex_direction"), "missing flex_direction in: {code}");
}

/// Test that codegen handles Box props correctly
#[test]
fn test_codegen_box_props() {
    use crate::codegen::jsx::generate_widget_for_jsx;

    let box_jsx = serde_json::json!({
        "kind": "JSX",
        "opening": {
            "name": { "Ident": "Box" },
            "attrs": [
                { "Attr": { "name": "padding", "value": { "Number": 2.0 } } },
                { "Attr": { "name": "margin", "value": { "Number": 1.0 } } },
                { "Attr": { "name": "gap", "value": { "Number": 3.0 } } }
            ],
            "self_closing": false
        },
        "children": [{ "kind": "Text", "text": "Content" }]
    });

    let widget = generate_widget_for_jsx(box_jsx);
    assert!(widget.is_some(), "should generate widget with props");
}

/// Test that codegen handles Text props correctly
#[test]
fn test_codegen_text_props() {
    use crate::codegen::jsx::generate_widget_for_jsx;

    let text_jsx = serde_json::json!({
        "kind": "JSX",
        "opening": {
            "name": { "Ident": "Text" },
            "attrs": [
                { "Attr": { "name": "bold", "value": { "Bool": true } } },
                { "Attr": { "name": "color", "value": { "String": "green" } } }
            ],
            "self_closing": false
        },
        "children": [{ "kind": "Text", "text": "Styled" }]
    });

    let widget = generate_widget_for_jsx(text_jsx);
    assert!(widget.is_some(), "should generate widget with text props");
}

/// Test expr_to_rust handles Ident
#[test]
fn test_expr_to_rust_ident() {
    let expr = json!({"Ident": {"name": "myVar"}});
    let result = expr_value_to_rust(&expr);
    assert!(result.is_some(), "should convert Ident to Rust");
    assert!(result.unwrap().contains("myVar"), "should contain variable name");
}

/// Test expr_to_rust handles Number
#[test]
fn test_expr_to_rust_number() {
    let expr = json!({"Number": 42.5});
    let result = expr_value_to_rust(&expr);
    assert!(result.is_some(), "should convert Number to Rust");
    assert!(result.unwrap().contains("42"), "should contain number");
}

/// Test expr_to_rust handles String
#[test]
fn test_expr_to_rust_string() {
    let expr = json!({"String": "hello"});
    let result = expr_value_to_rust(&expr);
    assert!(result.is_some(), "should convert String to Rust");
}

/// Test expr_to_rust handles Bool
#[test]
fn test_expr_to_rust_bool() {
    let expr_true = json!({"Bool": true});
    let result_true = expr_value_to_rust(&expr_true);
    assert!(result_true.is_some(), "should convert Bool true");

    let expr_false = json!({"Bool": false});
    let result_false = expr_value_to_rust(&expr_false);
    assert!(result_false.is_some(), "should convert Bool false");
}

/// Test expr_to_rust handles conditional
#[test]
fn test_expr_to_rust_conditional() {
    let expr = json!({
        "Cond": {
            "test": {"Ident": {"name": "x"}},
            "consequent": {"Number": 1.0},
            "alternate": {"Number": 2.0}
        }
    });

    let result = expr_value_to_rust(&expr);
    assert!(result.is_some(), "should convert conditional to Rust");
}

/// Test extract_var_declarations handles multiple declarations
#[test]
fn test_extract_var_declarations_multiple() {
    let body = json!([
        {
            "kind": "Expr",
            "expr": {
                "Assign": {
                    "op": "Assign",
                    "left": { "Ident": { "name": "count" } },
                    "right": { "Number": 0.0 }
                }
            }
        },
        {
            "kind": "Expr",
            "expr": {
                "Assign": {
                    "op": "Assign",
                    "left": { "Ident": { "name": "name" } },
                    "right": { "String": "test" }
                }
            }
        },
        {
            "kind": "Return",
            "arg": { "kind": "String", "0": "test" }
        }
    ]);

    let decls = extract_var_declarations(&body);
    assert_eq!(decls.len(), 2, "should extract two declarations");
}

/// Test that Box children are properly extracted
#[test]
fn test_box_children_extraction() {
    use crate::codegen::jsx::extract_jsx_from_function_with_vars;

    let func_item = json!({
        "Decl": {
            "Function": {
                "name": "BoxChildren",
                "body": [
                    {
                        "kind": "Return",
                        "arg": {
                            "JSX": {
                                "opening": { "name": { "Ident": "Box" }, "attrs": [], "self_closing": false },
                                "closing": { "name": { "Ident": "Box" } },
                                "children": [
                                    { "kind": "Text", "text": "Child 1" },
                                    { "kind": "Text", "text": "Child 2" },
                                    { "kind": "Text", "text": "Child 3" }
                                ]
                            }
                        }
                    }
                ]
            }
        }
    });

    let result = extract_jsx_from_function_with_vars(&func_item);
    assert!(result.is_some(), "should extract JSX");
    let (jsx, _) = result.unwrap();

    // Check that children are extracted
    let children = jsx.get("children").expect("should have children");
    assert!(children.is_array(), "children should be an array");
    let children_arr = children.as_array().unwrap();
    assert_eq!(children_arr.len(), 3, "should have 3 children");
}
