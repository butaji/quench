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
        let hir: runts_plugin::hir::Module = serde_json::from_str(hir_str).map_err(|e| {
            PluginError::codegen("ratatui", "unknown", format!("failed to parse HIR: {e}"))
        })?;
        let source_path = hir.source_path.as_deref().unwrap_or("unknown");
        if let Some(items_json) = &hir.items_json {
            if let Some(code) = self.try_codegen_jsx(items_json) {
                return Ok(code);
            }
        }
        self.codegen_stub_with_source(source_path)
    }
    fn cargo_deps(&self) -> Vec<CargoDep> {
        let mut deps = Vec::new();
        deps.push(CargoDep {
            name: "ratatui".to_string(),
            version: Some("0.26".to_string()),
            path: None,
            features: vec!["crossterm".to_string()],
        });
        deps.push(CargoDep {
            name: "crossterm".to_string(),
            version: Some("0.27".to_string()),
            path: None,
            features: vec![],
        });
        deps.push(CargoDep {
            name: "anyhow".to_string(),
            version: Some("1.0".to_string()),
            path: None,
            features: vec![],
        });
        // The Ink-style JSX tags (`<Box>`, `<Text>`,
        // `<Newline>`, `<Spacer>`, `<Static>`,
        // `<Transform>`) compile to calls into the
        // `runts-ink` crate. Path is resolved by
        // `find_runts_ink_path` so the dep works
        // whether the build dir is a temp dir or a
        // persistent `.runts/build` inside the repo.
        deps.push(CargoDep {
            name: "runts-ink".to_string(),
            version: None,
            path: Some(find_runts_ink_path()),
            features: vec![],
        });
        deps
    }

    fn codegen_entry(&self, modules: &[runts_plugin::hir::Module]) -> Result<String, PluginError> {
        // allow:complexity
        // allow:too_many_lines
        // Try the new runts-ink JSX codegen first.
        if let Some(code) = self.first_ink_codegen(modules) {
            return Ok(code);
        }

        // Aggregate widgets from all modules and
        // generate a proper entry point.
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

    fn dev_run_once(&self, _state: &mut dyn DevState) -> Result<DevAction, PluginError> {
        // For TUI apps, dev_run_once returns Stop since the TUI takes over the event loop.
        // This tells the dev server to stop calling this method and let the TUI run.
        Ok(DevAction::Stop)
    }

    fn dev_reload(&self, _ctx: &mut DevContext, _state: &mut dyn DevState) -> Result<(), PluginError> {
        Ok(())
    }
}

/// Fallback stub when no JSX is detected.
impl RatatuiPlugin {
    /// Try lowering the first module's HIR to a
    /// real `runts_ink::Box`/`Text` VNode expression
    /// via `runts_ink::render_to_string`. This is
    /// the 3-environment path: same `.tsx` source
    /// runs in Deno+Ink, `runts dev --ink`, and
    /// `runts build --plugin ratatui`.
    fn first_ink_codegen(&self, modules: &[runts_plugin::hir::Module]) -> Option<String> {
        for module in modules {
            if let Some(items_json) = &module.items_json {
                if let Some(code) = crate::codegen::try_codegen_jsx(items_json) {
                    return Some(code);
                }
            }
        }
        None
    }

    fn codegen_stub_with_source(&self, source_path: &str) -> Result<String, PluginError> {
        // Fallback emitted when no JSX was found in
        // the HIR. We still produce a runts-ink
        // binary (with a placeholder Text node) so
        // the generated `main` matches the shape of
        // the JSX path and the cargo build links the
        // same `runts-ink` runtime.
        let code = quote! {
            //! Fallback Ink entry: generated by runts-ratatui 0.1.
            //!
            //! No JSX was detected in the HIR for
            //! `#source_path`. Emits a placeholder
            //! Text so the build still produces a
            //! runnable binary that links
            //! `runts-ink`.

            use runts_ink;

            fn main() -> anyhow::Result<()> {
                let root: runts_ink::VNode =
                    runts_ink::Text::new(
                        String::from("Ratatui widget (source: ")
                            + #source_path
                            + ")",
                    )
                    .into();
                let rendered = runts_ink::render_to_string(
                    root,
                    runts_ink::RenderOptions::default(),
                )?;
                print!("{rendered}");
                Ok(())
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

/// Locate the `runts-ink` crate on disk. Searches
/// relative to the running `runts` binary (so it
/// works for `target/debug/runts` and
/// `target/release/runts`), then relative to the
/// current working directory. Returns an absolute,
/// canonicalized path. Used by `cargo_deps` to add
/// `runts-ink` as a path dep in the generated
/// `Cargo.toml`.
fn find_runts_ink_path() -> std::path::PathBuf {
    let rel = "crates/runts-ink";
    // 1. Walk up from the `runts` exe dir.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(hit) = find_ancestor_with(&exe, rel) {
            return hit;
        }
    }
    // 2. Walk up from the current working directory.
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(hit) = find_ancestor_with(&cwd, rel) {
            return hit;
        }
    }
    // 3. Last resort: leave it as a relative path
    // and let cargo fail with a helpful message.
    std::path::PathBuf::from(rel)
}

/// Walk `start`'s ancestors, return the canonicalized
/// `dir.join(rel)` whose `Cargo.toml` exists, or None.
fn find_ancestor_with(start: &std::path::Path, rel: &str) -> Option<std::path::PathBuf> {
    for dir in start.ancestors() {
        let candidate = dir.join(rel);
        if candidate.join("Cargo.toml").exists() {
            return Some(candidate.canonicalize().unwrap_or(candidate));
        }
    }
    None
}
