//! Ratatui plugin implementation - real widget codegen from HIR.
//!
//! Parses TSX HIR JSON and converts JSX elements to Ratatui widget code.

use quote::quote;

use runts_plugin::{
    CargoDep, DevAction, DevContext, DevState, Plugin, PluginError,
};

use crate::codegen;

/// Ratatui widget codegen from HIR JSON.
///
/// Maps JSX tags to Ratatui widgets:
/// - `<text>` → `Paragraph::new(...)`
/// - `<block title="..." borders={true}>...</block>` → `Block::default().title(...).borders(...)`
/// - `<row>` / `<col>` → `Layout` with direction
impl RatatuiPlugin {
    /// Try to generate widget code from HIR items JSON.
    /// Returns Some(code) if JSX was detected, None otherwise.
    pub(crate) fn try_codegen_jsx(&self, items: &serde_json::Value) -> Option<String> {
        codegen::try_codegen_jsx(items)
    }
}

impl Plugin for RatatuiPlugin {
    fn name(&self) -> &str {
        "ratatui"
    }

    fn help_text(&self) -> &str {
        "Ratatui TUI framework"
    }

    fn codegen_module(&self, hir_str: &str) -> Result<String, PluginError> {
        let hir: runts_plugin::hir::Module = serde_json::from_str(hir_str)
            .map_err(|e| PluginError::codegen("ratatui", "unknown", format!("failed to parse HIR: {e}")))?;
        if let Some(items_json) = &hir.items_json {
            if let Some(code) = self.try_codegen_jsx(items_json) {
                return Ok(code);
            }
        }
        self.codegen_stub()
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
        let app_body = crate::codegen::widget_text("Hello from Ratatui!");
        let entry = crate::codegen::tui_main(app_body);
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

/// Fallback stub when no JSX is detected.
impl RatatuiPlugin {
    fn codegen_stub(&self) -> Result<String, PluginError> {
        let code = quote! {
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
}

pub struct RatatuiPlugin;

struct RatatuiDevState;

impl DevState for RatatuiDevState {}
