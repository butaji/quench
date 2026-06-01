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
    serde_json::json!({
        "type": "Decl",
        "Decl": {
            "kind": "Function",
            "name": name,
            "body": { "Block": { "stmts": [{ "kind": "Return", "arg": body }] } }
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
    let result = plugin.codegen_module(r#"{"type":"Text","props":{"text":"hi"}}"#);
    assert!(result.is_ok());
    let code = result.unwrap();
    assert!(code.contains("pub fn render"));
    assert!(code.contains("Paragraph :: new"));
    // Fallback now includes source trace
    assert!(code.contains("source:"));
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
    assert_eq!(deps.len(), 3);
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
    assert!(code.contains("Paragraph::new"), "{}", code);
    assert!(code.contains("Hello World"), "{}", code);
}

#[test]
fn test_try_codegen_jsx_block_with_title() {
    let plugin = ratatui_plugin();
    let block = jsx_block_with_title("My App", serde_json::json!({ "kind": "Text", "text": "Content" }));
    let items = items_with_fn("App", block);
    let result = plugin.try_codegen_jsx(&items);
    assert!(result.is_some());
    let code = normalize(&result.unwrap());
    assert!(code.contains("Block::default"), "{}", code);
    assert!(code.contains("title"), "{}", code);
    assert!(code.contains("My App"), "{}", code);
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
    assert!(code.contains("Layout::default"), "{}", code);
    assert!(code.contains("Horizontal"), "{}", code);
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
    assert!(code.contains("Paragraph::new"), "{}", code);
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
    assert!(code.contains("Layout::default"), "{}", code);
    assert!(code.contains("Vertical"), "{}", code);
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