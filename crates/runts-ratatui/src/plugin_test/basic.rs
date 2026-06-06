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
