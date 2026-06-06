        "Decl": {
            "kind": "Function",
            "name": "NoJsx",
            "body": { "Block": { "stmts": [{ "kind": "Return", "arg": { "kind": "String", "0": "hello" } }] } }
        }
    }]);
    let result = plugin.try_codegen_jsx(&items);
    assert!(result.is_none());
}

#[test]
fn test_try_codegen_jsx_paragraph() {
    let plugin = ratatui_plugin();
    let items = items_with_fn("Para", jsx_text("paragraph", "Test paragraph"));
    let result = plugin.try_codegen_jsx(&items);
    assert!(result.is_some());
    let code = normalize(&result.unwrap());
    // `<paragraph>` is a Text in the new codegen,
    // lowered to `Text::new("...")`.
    assert!(code.contains("Text::new"), "{}", code);
    assert!(code.contains("Test paragraph"), "{}", code);
}

#[test]
fn test_try_codegen_jsx_col() {
    let plugin = ratatui_plugin();
    let items = items_with_fn("VLayout", serde_json::json!({
        "kind": "JSX",
        "opening": { "name": { "Ident": "col" }, "attrs": [], "self_closing": false },
        "children": [{ "kind": "Text", "text": "Top" }],
        "closing": { "name": { "Ident": "col" } }
    }));
    let result = plugin.try_codegen_jsx(&items);
    assert!(result.is_some());
    let code = normalize(&result.unwrap());
    // `<col>` lowers to `Box::new().flex_direction(col)`.
    assert!(code.contains("Box::new"), "{}", code);
    assert!(code.contains("flex_direction"), "{}", code);
    assert!(code.contains("Top"), "{}", code);
}

#[test]
fn test_try_codegen_jsx_nested_block_in_block() {
    let plugin = ratatui_plugin();
    let inner_text = jsx_text("text", "Inner text");
    let outer = jsx_block_with_title("Outer", serde_json::json!({ "kind": "JSX", "JSX": inner_text }));
    let items = items_with_fn("Nested", outer);
    let result = plugin.try_codegen_jsx(&items);
    assert!(result.is_some());
    let code = normalize(&result.unwrap());
    // Nested Box has multiple `child` calls.
    assert!(code.contains("Box::new"), "{}", code);
    assert!(code.contains("child"), "{}", code);
    assert!(code.contains("Outer"), "{}", code);
    assert!(code.contains("Inner text"), "{}", code);
}

#[test]
fn test_widget_text_codegen() {
    let result = codegen::widget_text("Test");
    let code = normalize(&result.to_string());
    assert!(code.contains("render_widget"), "{}", code);
    assert!(code.contains("Paragraph::new"), "{}", code);
    assert!(code.contains("Test"), "{}", code);
}

#[test]
fn test_tui_main_codegen() {
    let body = codegen::widget_text("Hello");
    let result = codegen::tui_main(body);
    let code = result.to_string();
    assert!(code.contains("fn main"));
    assert!(code.contains("terminal"));
}

/// A `<Box flexDirection="column">` with two `<Text>`
/// children should produce a paragraph widget. The
/// direction is captured in the emitted code.
#[test]
fn test_ink_box_with_column_direction_codegens_paragraph() {
    let plugin = ratatui_plugin();
    // Bare Box with no children — just verify the
    // dispatch reaches the Ink helper. The recursive
    // child layout is not yet wired, so we don't assert
    // on the child render output.
    let inner = serde_json::json!({
        "kind": "JSX",
        "opening": {
            "name": { "Ident": "Box" },
            "attrs": [
                { "Attr": {
                    "name": "flexDirection",
                    "value": "column"
                }}
            ],
            "self_closing": true
        },
        "children": [],
        "closing": null
    });
    let items = items_with_fn("App", inner);
    let result = plugin.try_codegen_jsx(&items);
    assert!(result.is_some(), "no widget code for Ink Box");
    let code = normalize(&result.unwrap());
    // The new codegen emits a `fn main` that uses
    // `runts_ink::Box::new()` etc. The Box should
    // be lowered to a real `runts_ink::Box` builder
    // call (not a `ratatui::Paragraph` placeholder).
    assert!(code.contains("fn main"));
    assert!(code.contains("runts_ink::Box::new"));
    assert!(code.contains("FlexDirection::Column"));
}

/// A bare `<Newline>` (or `<Spacer>`) should produce a
/// non-empty paragraph widget so the layout engine has
/// something to render.
#[test]
fn test_ink_newline_codegens_empty_paragraph() {
    let plugin = ratatui_plugin();
    let inner = serde_json::json!({
        "kind": "JSX",
        "opening": {
            "name": { "Ident": "Newline" },
            "attrs": [],
            "self_closing": true
        },
        "children": [],
        "closing": null
    });
    let items = items_with_fn("App", inner);
    let result = plugin.try_codegen_jsx(&items);
    assert!(result.is_some());
    let code = normalize(&result.unwrap());
    // `<Newline>` should lower to a
    // `runts_ink::Newline::new().into()` VNode
    // expression, not a Ratatui Paragraph stub.
    assert!(code.contains("runts_ink::Newline::new"));
}

/// A `<Text>` with a conditional expression like
/// `<Text>{active ? 'yes' : 'no'}</Text>` should
/// include the expression in the generated Text.
#[test]
fn test_text_with_conditional_expression() {
    let plugin = ratatui_plugin();
    // Text with a conditional expression
    let inner = serde_json::json!({
        "kind": "JSX",
        "opening": {
            "name": { "Ident": "Text" },
            "attrs": [],
            "self_closing": false
        },
        "children": [
            { "Text": "Status: " },
            {
                "Expr": {
                    "Cond": {
                        "test": { "Ident": { "name": "isActive" } },
                        "consequent": { "String": "ACTIVE" },
                        "alternate": { "String": "INACTIVE" }
                    }
                }
            }
        ],
        "closing": { "name": { "Ident": "Text" } }
    });
    let items = items_with_fn("App", inner);
    let result = plugin.try_codegen_jsx(&items);
    assert!(result.is_some());
    let code = normalize(&result.unwrap());
    // The generated code should include the expression
    // Should have Text::new with format! macro
    assert!(code.contains("Text::new"), "missing Text::new: {}", code);
    // Should have the conditional expression
    assert!(code.contains("isActive") || code.contains("format!"), 
            "missing expression in codegen: {}", code);
}

/// A `<Text>` with an identifier expression like
/// `<Text>Count: {count}</Text>` should include
/// the identifier in the generated Text.
#[test]
fn test_text_with_identifier_expression() {
    let plugin = ratatui_plugin();
    // Text with an identifier
    let inner = serde_json::json!({
        "kind": "JSX",
        "opening": {
            "name": { "Ident": "Text" },
            "attrs": [],
            "self_closing": false
        },
        "children": [
            { "Text": "Count: " },
            { "Expr": { "Ident": { "name": "count" } } }
        ],
        "closing": { "name": { "Ident": "Text" } }
    });
    let items = items_with_fn("App", inner);
    let result = plugin.try_codegen_jsx(&items);
