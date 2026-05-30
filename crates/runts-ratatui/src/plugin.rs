//! Ratatui plugin implementation.
//!
//! # v0.1
//! `codegen_module` returns a hardcoded stub. Widget codegen infrastructure
//! (`generate_widget_code`, etc.) is preserved but unused — HIR JSON format
//! from TSX does not match the expected `{type, props, children}` schema.
//!
//! # v0.2
//! Wire real widget codegen by traversing HIR JSON (like Fresh plugin does)
//! or by adapting the widget functions to the actual HIR format.

use runts_plugin::{
    CargoDep, DevAction, DevContext, DevState, Plugin, PluginError,
};

use crate::codegen;

// ─── Dead code (preserved for v0.2 widget codegen wiring) ───────────────────

/// HIR widget node from JSX-like source
#[allow(dead_code)]
#[derive(Debug, serde::Deserialize)]
struct WidgetNode {
    #[serde(rename = "type")]
    widget_type: String,
    props: Option<WidgetProps>,
    children: Option<Vec<WidgetNode>>,
}

/// Widget properties
#[allow(dead_code)]
#[derive(Debug, serde::Deserialize)]
struct WidgetProps {
    title: Option<String>,
    borders: Option<bool>,
    direction: Option<String>,
    #[serde(default)]
    text: String,
}

/// Parse HIR string and generate widget code
#[allow(dead_code)]
fn generate_widget_code(hir_str: &str) -> Result<proc_macro2::TokenStream, PluginError> {
    let nodes: Vec<WidgetNode> = serde_json::from_str(hir_str)
        .or_else(|_| serde_json::from_str(&format!("[{}]", hir_str)))
        .map_err(|e| PluginError::codegen("ratatui", "unknown", format!("failed to parse HIR: {e}")))?;

    if nodes.is_empty() {
        return Ok(codegen::widget_text(""));
    }

    generate_widget_from_node(&nodes[0])
}

#[allow(dead_code)]
fn generate_widget_from_node(node: &WidgetNode) -> Result<proc_macro2::TokenStream, PluginError> {
    match node.widget_type.as_str() {
        "Block" => {
            let title = node.props.as_ref().and_then(|p| p.title.clone());
            let borders = node.props.as_ref().map(|p| p.borders.unwrap_or(false)).unwrap_or(false);
            let children_code = generate_children_code(&node.children)?;
            Ok(codegen::widget_block(title.as_deref(), borders, children_code))
        }
        "Text" => {
            let text = node.props.as_ref().map(|p| p.text.clone()).unwrap_or_default();
            Ok(codegen::widget_text(&text))
        }
        "Layout" => {
            let direction = node.props.as_ref().and_then(|p| p.direction.clone()).unwrap_or_else(|| "vertical".to_string());
            let children: Vec<proc_macro2::TokenStream> = node.children
                .iter()
                .flatten()
                .filter_map(|c| generate_widget_from_node(c).ok())
                .collect();
            Ok(codegen::widget_layout(&direction, children))
        }
        _ => Ok(codegen::widget_text(&node.widget_type)),
    }
}

#[allow(dead_code)]
fn generate_children_code(children: &Option<Vec<WidgetNode>>) -> Result<proc_macro2::TokenStream, PluginError> {
    match children {
        Some(ref kids) if !kids.is_empty() => {
            let parts: Vec<proc_macro2::TokenStream> = kids
                .iter()
                .filter_map(|c| generate_widget_from_node(c).ok())
                .collect();
            if parts.len() == 1 {
                Ok(parts[0].clone())
            } else {
                Ok(quote::quote! { #( #parts )* })
            }
        }
        _ => Ok(codegen::widget_text("")),
    }
}

impl Plugin for RatatuiPlugin {
    fn name(&self) -> &str {
        "ratatui"
    }

    fn help_text(&self) -> &str {
        "Ratatui TUI framework"
    }

    fn codegen_module(&self, _hir_str: &str) -> Result<String, PluginError> {
        // HIR parsing doesn't work with standard TSX HIR format (no type/props/children fields).
        // For now, return a stub render function that compiles.
        // Architecture: widget functions generate RENDER STATEMENTS, not widget expressions.
        let code = quote::quote! {
            use ratatui::prelude::*;

            pub fn render(frame: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect) {
                frame.render_widget(
                    ratatui::widgets::Paragraph::new("Hello from Ratatui!"),
                    area,
                );
            }
        };
        Ok(code.to_string())
    }

    fn cargo_deps(&self) -> Vec<CargoDep> {
        vec![
            CargoDep {
                name: "ratatui".to_string(),
                version: Some("0.26".to_string()),
                path: None,
                features: vec!["crossterm".to_string()],
            },
            CargoDep {
                name: "crossterm".to_string(),
                version: Some("0.27".to_string()),
                path: None,
                features: vec![],
            },
            CargoDep {
                name: "anyhow".to_string(),
                version: Some("1.0".to_string()),
                path: None,
                features: vec![],
            },
        ]
    }

    fn codegen_entry(&self, _modules: &[runts_plugin::hir::Module]) -> Result<String, PluginError> {
        // Generate render statements directly - no widget wrapper needed
        let app_body = codegen::widget_text("Hello from Ratatui!");
        let entry = codegen::tui_main(app_body);
        Ok(entry.to_string())
    }

    fn dev_init(&self, _ctx: &mut DevContext) -> Result<Box<dyn DevState>, PluginError> {
        Ok(Box::new(RatatuiDevState))
    }

    fn dev_run_once(&self, _state: &mut dyn DevState) -> Result<DevAction, PluginError> {
        Ok(DevAction::Continue)
    }

    fn dev_reload(&self, _ctx: &mut DevContext, _state: &mut dyn DevState) -> Result<(), PluginError> {
        Ok(())
    }
}

pub struct RatatuiPlugin;

struct RatatuiDevState;

impl DevState for RatatuiDevState {}