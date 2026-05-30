//! Tests for the Ratatui plugin.

use runts_plugin::Plugin;

use crate::RatatuiPlugin;

fn ratatui_plugin() -> RatatuiPlugin {
    RatatuiPlugin
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
    // Stub should compile as valid Rust (quote adds spaces around ::)
    assert!(code.contains("pub fn render"));
    assert!(code.contains("Paragraph :: new"));
    assert!(code.contains("Hello from Ratatui!"));
}

#[test]
fn test_codegen_entry_generates_main() {
    let plugin = ratatui_plugin();
    let result = plugin.codegen_entry(&[]);
    assert!(result.is_ok());
    let code = result.unwrap();
    // Entry should contain main function (quote adds spaces around ::)
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
    let mut ctx = runts_plugin::DevContext {
        root: std::path::PathBuf::from("/tmp"),
        modules: vec![],
    };
    let result = plugin.dev_init(&mut ctx);
    assert!(result.is_ok());
    let _state = result.unwrap();
}

#[test]
fn test_dev_run_once_returns_continue() {
    use runts_plugin::{DevAction, DevState};

    struct TestDevState;
    impl DevState for TestDevState {}

    let plugin = ratatui_plugin();
    let mut state = TestDevState;
    let result = plugin.dev_run_once(&mut state);
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), DevAction::Continue));
}

#[test]
fn test_dev_reload_returns_ok() {
    use runts_plugin::DevState;

    struct TestDevState;
    impl DevState for TestDevState {}

    let plugin = ratatui_plugin();
    let mut ctx = runts_plugin::DevContext {
        root: std::path::PathBuf::from("/tmp"),
        modules: vec![],
    };
    let mut state = TestDevState;
    let result = plugin.dev_reload(&mut ctx, &mut state);
    assert!(result.is_ok());
}
