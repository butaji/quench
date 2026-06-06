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
    // Opaque JSON that's not a valid HIR (wrong
    // shape) — exercises the codegen-stub fallback
    // path. The fallback emits a `Hello from
    // Ratatui!` placeholder wrapped in the runts
    // Ink codegen module wrapper.
    let result = plugin.codegen_module(r#"{"type":"Text","props":{"text":"hi"}}"#);
