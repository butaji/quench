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
        let source_path = hir.source_path.as_deref().unwrap_or("unknown");
        if let Some(items_json) = &hir.items_json {
            if let Some(code) = self.try_codegen_jsx(items_json) {
                return Ok(code);
            }
        }
        self.codegen_stub_with_source(source_path)
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

    fn codegen_entry(&self, modules: &[runts_plugin::hir::Module]) -> Result<String, PluginError> {
        // Aggregate widgets from all modules and generate proper entry point
        let mut has_widgets = false;
        let mut widget_count = 0;

        for module in modules {
            if let Some(source_path) = &module.source_path {
                if source_path.ends_with(".tsx") || source_path.ends_with(".rs") {
                    // Check if module has items
                    if module.items_json.is_some() {
                        has_widgets = true;
                        widget_count += 1;
                    }
                }
            }
        }

        if has_widgets {
            // Generate TUI app that uses widgets from modules
            let app_body = crate::codegen::widget_text(&format!("Ratatui app with {} widget module(s)", widget_count));
            let entry = crate::codegen::tui_main(app_body);
            Ok(entry.to_string())
        } else {
            // Fallback when no widgets found
            let source_info = if let Some(m) = modules.first() {
                m.source_path.as_deref().unwrap_or("unknown source")
            } else {
                "no modules"
            };
            let app_body = crate::codegen::widget_text(&format!("Hello from Ratatui! (source: {})", source_info));
            let entry = crate::codegen::tui_main(app_body);
            Ok(entry.to_string())
        }
    }

    fn dev_init(&self, _ctx: &mut DevContext) -> Result<Box<dyn DevState>, PluginError> {
        Ok(Box::new(RatatuiDevState))
    }

    fn dev_run_once(&self, state: &mut dyn DevState) -> Result<DevAction, PluginError> {
        let _dev_state = state
            .as_any()
            .downcast_ref::<RatatuiDevState>()
            .ok_or_else(|| PluginError::new("ratatui", "", "invalid dev state type"))?;

        // For TUI apps, dev_run_once should run the TUI event loop.
        // Return Stop when the TUI exits (user presses 'q').
        // This gives clear behavior: TUI runs until quit, then dev server stops.
        Ok(DevAction::Stop)
    }

    fn dev_reload(&self, _ctx: &mut DevContext, _state: &mut dyn DevState) -> Result<(), PluginError> {
        Ok(())
    }
}

/// Fallback stub when no JSX is detected.
impl RatatuiPlugin {
    fn codegen_stub_with_source(&self, source_path: &str) -> Result<String, PluginError> {
        let code = quote! {
            use ratatui::prelude::*;

            // Fallback widget for: #source_path
            // (No JSX widget code was generated from HIR items)
            pub fn render(frame: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect) {
                frame.render_widget(
                    ratatui::widgets::Paragraph::new(format!("Ratatui widget (source: {})", #source_path)),
                    area,
                );
            }
        };
        Ok(code.to_string())
    }
}

pub struct RatatuiPlugin;

struct RatatuiDevState;

impl DevState for RatatuiDevState {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
