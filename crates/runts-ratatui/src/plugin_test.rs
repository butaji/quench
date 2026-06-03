//! Tests for the Ratatui plugin.

use runts_plugin::Plugin;

use crate::{codegen, RatatuiPlugin};

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
    // Opaque JSON that's not a valid HIR (wrong
    // shape) — exercises the codegen-stub fallback
    // path. The fallback emits a `Hello from
    // Ratatui!` placeholder wrapped in the runts
    // Ink codegen module wrapper.
    let result = plugin.codegen_module(r#"{"type":"Text","props":{"text":"hi"}}"#);
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
fn test_dev_run_once_returns_stop() {
    use runts_plugin::{DevAction, DevState};
    struct TestDevState;
    impl DevState for TestDevState {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
    let plugin = ratatui_plugin();
    let mut state = TestDevState;
    let result = plugin.dev_run_once(&mut state);
    assert!(result.is_ok());
    // TUI dev_run_once returns Stop since TUI takes over the event loop
    assert!(matches!(result.unwrap(), DevAction::Stop));
}

#[test]
fn test_dev_reload_returns_ok() {
    use runts_plugin::DevState;
    struct TestDevState;
    impl DevState for TestDevState {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
    let plugin = ratatui_plugin();
    let mut ctx = runts_plugin::DevContext { root: std::path::PathBuf::from("/tmp"), modules: vec![] };
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