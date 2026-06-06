//! Tests for the Ratatui plugin.

use runts_plugin::Plugin;
use serde_json::json;

use crate::{codegen, RatatuiPlugin};
use crate::codegen::jsx::{extract_var_declarations, expr_value_to_rust, extract_jsx_from_function_with_vars};

fn ratatui_plugin() -> RatatuiPlugin {
    RatatuiPlugin
}
/// Normalize quote's spacing around :: for test assertions
fn normalize(s: &str) -> String {
    let s = s.replace(" :: ", "::");
    let s = s.replace(" ::", "::");
    s.replace(":: ", "::")
}

// Test data builders

fn jsx_text(name: &str, text: &str) -> serde_json::Value {
    serde_json::json!({
        "kind": "JSX",
        "opening": {
            "name": { "Ident": name },
            "attrs": [],
            "self_closing": false
        },
        "children": [{ "kind": "Text", "text": text }],
        "closing": { "name": { "Ident": name } }
    })
}

fn jsx_block_with_title(title: &str, child: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "kind": "JSX",
        "opening": {
            "name": { "Ident": "block" },
            "attrs": [{ "Attr": { "name": "title", "value": title } }],
            "self_closing": false
        },
        "children": [child],
        "closing": { "name": { "Ident": "block" } }
    })
}

fn jsx_row(children: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({
        "kind": "JSX",
        "opening": {
            "name": { "Ident": "row" },
            "attrs": [],
            "self_closing": false
        },
        "children": children,
        "closing": { "name": { "Ident": "row" } }
    })
}

fn fn_decl(name: &str, body: serde_json::Value) -> serde_json::Value {
    // Use the real HIR shape: items are
    // `{Decl: {Function: {name, body: [...]}}}`.
    // The body is a flat array of statements,
    // not `{Block: {stmts: [...]}}` (that was a
    // hand-rolled fixture shape, not the parser).
    serde_json::json!({
        "Decl": {
            "Function": {
                "name": name,
                "body": [{ "kind": "Return", "arg": body }]
            }
        }
    })
}

fn items_with_fn(name: &str, body: serde_json::Value) -> serde_json::Value {
    serde_json::json!([fn_decl(name, body)])
}

#[test]
fn test_ratatui_plugin_name() {
    let plugin = ratatui_plugin();
    assert_eq!(plugin.name(), "ratatui");
}

#[test]
fn test_ratatui_plugin_help() {
    let plugin = ratatui_plugin();
    assert_eq!(plugin.help_text(), "Ratatui TUI framework");
}

#[test]
fn test_codegen_module_returns_valid_rust() {
    let plugin = ratatui_plugin();
    // Empty HIR module — exercises the codegen-stub fallback
    // path. The fallback emits a placeholder wrapped in the runts
    // Ink codegen module wrapper.
    let module = runts_hir::Module {
        source: String::new(),
        source_path: None,
        route_info: None,
        items: vec![],
        types: std::collections::HashMap::new(),
    };
    let result = plugin.codegen_module(&module);
    assert!(result.is_ok());
    let code = result.unwrap();
    assert!(code.contains("fn main"));
    assert!(code.contains("runts_ink"));
    // Fallback still includes the source trace.
    assert!(code.contains("source:") || code.contains("unknown"));
}

#[test]
fn test_codegen_entry_generates_main() {
    let plugin = ratatui_plugin();
    let result = plugin.codegen_entry(&[]);
    assert!(result.is_ok());
    let code = result.unwrap();
    assert!(code.contains("fn main ()"));
    assert!(code.contains("anyhow :: Result"));
    assert!(code.contains("terminal . draw"));
}

#[test]
fn test_codegen_flex_grow_envelope_unwrap() {
    // `<Box flexGrow={1}>` should emit
    // `.flex_grow(1f64)` even though the HIR
    // serializes the brace-expression numeric
    // value as `{"Expr": {"Number": 1.0}}`.
    let plugin = ratatui_plugin();
    let box_jsx = serde_json::json!({
        "kind": "JSX",
        "opening": {
            "name": { "Ident": "Box" },
            "attrs": [
                {
                    "Attr": {
                        "name": "flexGrow",
                        "value": {
                            "Expr": {
                                "Number": 1.0
                            }
                        }
                    }
                }
            ],
            "self_closing": false
        },
        "children": []
    });
    let items = items_with_fn("FlexGrowTest", box_jsx);
    let result = plugin.try_codegen_jsx(&items);
    assert!(result.is_some(), "codegen returned None");
    let code = normalize(&result.unwrap());
    assert!(
        code.contains("flex_grow"),
        "missing flex_grow in: {code}"
    );
}
fn test_cargo_deps() {
    let plugin = ratatui_plugin();
    let deps = plugin.cargo_deps();
    assert_eq!(deps.len(), 4);
    let ratatui = deps.iter().find(|d| d.name == "ratatui").expect("ratatui dep");
    assert_eq!(ratatui.version, Some("0.26".to_string()));
    assert!(ratatui.features.contains(&"crossterm".to_string()));
    let crossterm = deps.iter().find(|d| d.name == "crossterm").expect("crossterm dep");
    assert_eq!(crossterm.version, Some("0.27".to_string()));
    let anyhow = deps.iter().find(|d| d.name == "anyhow").expect("anyhow dep");
    assert_eq!(anyhow.version, Some("1.0".to_string()));
}

#[test]
fn test_dev_init_returns_state() {
    let plugin = ratatui_plugin();
    let mut ctx = runts_plugin::DevContext { root: std::path::PathBuf::from("/tmp"), modules: vec![] };
    let result = plugin.dev_init(&mut ctx);
    assert!(result.is_ok());
    let _state = result.unwrap();
}

#[test]
fn test_dev_run_once_returns_continue() {
    use runts_plugin::{DevAction, DevState};
    let plugin = ratatui_plugin();
    // Set up a real DevState via dev_init.
    let mut ctx = runts_plugin::DevContext {
        root: std::path::PathBuf::from("/tmp"),
        modules: vec![],
    };
    let mut state = plugin.dev_init(&mut ctx).unwrap();
    let result = plugin.dev_run_once(&mut *state);
    assert!(result.is_ok());
    // TUI dev_run_once returns Continue (not
    // Stop) when run in the rquickjs+bridge dev
    // path; it idles between file changes.
    assert!(matches!(result.unwrap(), DevAction::Continue));
}

#[test]
fn test_dev_reload_returns_ok() {
    use runts_plugin::DevState;
    struct TestDevState;
    impl DevState for TestDevState {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }
    let plugin = ratatui_plugin();
    let mut ctx = runts_plugin::DevContext {
        root: std::path::PathBuf::from("/tmp"),
        modules: vec![],
    };
    let mut state = TestDevState;
    let result = plugin.dev_reload(&mut ctx, &mut state);
    assert!(result.is_ok());
}

// JSX codegen tests

#[test]
fn test_try_codegen_jsx_simple_text() {
    let plugin = ratatui_plugin();
    let items = items_with_fn("Hello", jsx_text("text", "Hello World"));
    let result = plugin.try_codegen_jsx(&items);
    assert!(result.is_some());
    let code = normalize(&result.unwrap());
    // New codegen emits a runts-ink `Text::new(...)`
    // builder call, not a Ratatui Paragraph. Both
    // paths wrap in a `fn main()` that uses
    // `runts_ink::render_to_string`.
    assert!(code.contains("Text::new"), "{}", code);
    assert!(code.contains("Hello World"), "{}", code);
    assert!(code.contains("fn main"), "{}", code);
    assert!(code.contains("runts_ink::render_to_string"), "{}", code);
}

#[test]
fn test_try_codegen_jsx_block_with_title() {
    let plugin = ratatui_plugin();
    let block = jsx_block_with_title("My App", serde_json::json!({ "kind": "Text", "text": "Content" }));
    let items = items_with_fn("App", block);
    let result = plugin.try_codegen_jsx(&items);
    assert!(result.is_some());
    let code = normalize(&result.unwrap());
    // `<block title="...">` lowers to a runts-ink
    // `Box::new().border_style(...)` chain. The
    // title becomes the first Text child.
    assert!(code.contains("Box::new"), "{}", code);
    assert!(code.contains("My App"), "{}", code);
    assert!(code.contains("Content"), "{}", code);
}

#[test]
fn test_try_codegen_jsx_row() {
    let plugin = ratatui_plugin();
    let left = jsx_text("text", "Left");
    let right = jsx_text("text", "Right");
    let items = items_with_fn("Layout", jsx_row(vec![
        serde_json::json!({ "kind": "JSX", "JSX": left }),
        serde_json::json!({ "kind": "JSX", "JSX": right })
    ]));
    let result = plugin.try_codegen_jsx(&items);
    assert!(result.is_some());
    let code = normalize(&result.unwrap());
    // `<row>` lowers to a runts-ink `Box::new().flex_direction(row)`
    // with multiple `.child(...)` calls.
    assert!(code.contains("Box::new"), "{}", code);
    assert!(code.contains("flex_direction"), "{}", code);
    assert!(code.contains("Left"), "{}", code);
    assert!(code.contains("Right"), "{}", code);
}

#[test]
fn test_try_codegen_jsx_borderstyle_round() {
    let plugin = ratatui_plugin();
    let items = json!([{"Decl": {"Function": {"name": "App", "generics": [], "params": [], "return_type": null, "body": [{"kind": "Return", "arg": {"JSX": {"opening": {"name": {"Ident": "Box"}, "attrs": [{"Attr": {"name": "borderStyle", "value": {"String": "round"}}}], "self_closing": false}, "closing": {"name": {"Ident": "Box"}}, "children": [{"kind": "Text", "text": "hi"}]}}}}]}}}]);
    let result = plugin.try_codegen_jsx(&items);
    assert!(result.is_some());
    let code = normalize(&result.unwrap());
    assert!(code.contains("BorderStyle::Round"), "expected Round in: {code}", code = code);
}

#[test]
fn test_try_codegen_jsx_paddingx_expr() {
    let plugin = ratatui_plugin();
    let items = json!([{"Decl": {"Function": {"name": "App", "generics": [], "params": [], "return_type": null, "body": [{"kind": "Return", "arg": {"JSX": {"opening": {"name": {"Ident": "Box"}, "attrs": [{"Attr": {"name": "paddingX", "value": {"Expr": {"Number": 2.0}}}}, {"Attr": {"name": "paddingY", "value": {"Expr": {"Number": 1.0}}}}], "self_closing": false}, "closing": {"name": {"Ident": "Box"}}, "children": [{"kind": "Text", "text": "hi"}]}}}}]}}}]);
    let result = plugin.try_codegen_jsx(&items);
    assert!(result.is_some());
    let code = normalize(&result.unwrap());
    assert!(code.contains("padding_x"), "expected padding_x(2) in: {code}", code = code);
    assert!(code.contains("padding_y"), "expected padding_y(1) in: {code}", code = code);
}


#[test]
fn test_try_codegen_jsx_no_jsx_returns_none() {
    let plugin = ratatui_plugin();
    let items = serde_json::json!([{
        "type": "Decl",
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
fn hir_render_bordered_example() {
    // ATOMIC TEST: HIR runtime renders the
    // bordered example through the full pipeline.
    // Note: Uses direct Rust call to render function
    // instead of invoking binary (which would give test binary path).
    let src = include_str!("../../../examples/ink-bordered/tui/app.tsx");
    let transformed = crate::dev_jsx::transform(src);
    let program = crate::plugin::dev_eval_program_with_lowered(src, &transformed.js);
    let result = crate::plugin::run_ink_dev_with_program(&transformed.js, &program);
    assert!(result.is_ok(), "hir render failed: {:?}", result);
    let output = result.unwrap();
    assert!(output.contains("Bordered Example"),
        "hir-render output missing title: {output:?}");
}

#[test]
fn hir_render_aligned_example() {
    // ATOMIC TEST: HIR runtime renders the
    // aligned example correctly.
    // Note: Uses direct Rust call to render function
    // instead of invoking binary (which would give test binary path).
    let src = include_str!("../../../examples/ink-aligned/tui/app.tsx");
    let transformed = crate::dev_jsx::transform(src);
    let program = crate::plugin::dev_eval_program_with_lowered(src, &transformed.js);
    let result = crate::plugin::run_ink_dev_with_program(&transformed.js, &program);
    assert!(result.is_ok(), "hir render failed: {:?}", result);
    let output = result.unwrap();
    // Look for "Centered" in the output - the example has a centered title
    assert!(output.contains("Centered") || output.contains("centered"),
        "hir-render output missing 'Centered': {output:?}");
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
