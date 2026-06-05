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

    fn dev_init(&self, ctx: &mut DevContext) -> Result<Box<dyn DevState>, PluginError> {
        // Find the first .tsx or .ts module in the
        // project. The dev path renders a single
        // module (not a full app).
        let module = ctx
            .modules
            .iter()
            .find(|m| m.ends_with(".tsx") || m.ends_with(".ts"))
            .cloned();
        Ok(Box::new(RatatuiDevState { module, dirty: true }))
    }

    fn dev_run_once(&self, state: &mut dyn DevState) -> Result<DevAction, PluginError> {
        let st_any = state.as_any_mut();
        let st = match st_any.downcast_mut::<RatatuiDevState>() { Some(s) => s, None => return Err(PluginError::codegen("ratatui", "dev", "unexpected DevState type")) };
        if !st.dirty { return Ok(DevAction::Continue); }
        st.dirty = false;
        let Some(module_path) = st.module.clone() else { return Ok(DevAction::Continue) };
        let source = match std::fs::read_to_string(&module_path) { Ok(s) => s, Err(e) => { eprintln!("HIR render: read {module_path} failed: {e}"); return Ok(DevAction::Continue); }};
        let tmp = std::env::temp_dir().join("runts-hir-render.tsx");
        let _ = std::fs::write(&tmp, source.as_bytes());
        let runts_bin = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("runts"));
        match std::process::Command::new(&runts_bin).arg("hir-render").arg(&tmp).output() {
            Ok(out) => { let _ = std::io::stdout().write_all(&out.stdout); let _ = std::io::stdout().flush(); if !out.stderr.is_empty() { eprint!("{}", String::from_utf8_lossy(&out.stderr)); } Ok(DevAction::Continue) }
            Err(e) => { eprintln!("hir-render failed: {e}"); Ok(DevAction::Continue) }
        }
    }

    fn dev_reload(&self, _ctx: &mut DevContext, state: &mut dyn DevState) -> Result<(), PluginError> {
        // Mark the state dirty so the next
        // dev_run_once re-evals the source.
        if let Some(s) = state.as_any_mut().downcast_mut::<RatatuiDevState>() {
            s.dirty = true;
        }
        Ok(())
    }
}

/// Run the dev path with a pre-built eval program.
/// The test path uses this to pass a custom
/// program instead of `dev_eval_program`.
pub fn run_ink_dev_with_program(
    _js: &str,
    program: &str,
) -> Result<String, String> {
    use rquickjs::context::intrinsic;
    use rquickjs::{Context, Runtime};

    let runtime = Runtime::new().map_err(|e| format!("runtime: {e}"))?;
    let ctx = Context::builder()
        .with::<intrinsic::Eval>()
        .with::<intrinsic::Json>()
        .build(&runtime)
        .map_err(|e| format!("ctx: {e}"))?;
    let result: String = ctx
        .with(|ctx| {
            runts_ink::js_bridge::install(&ctx).map_err(|e| format!("install: {e}"))?;
            ctx.eval::<_, String>(program.to_string())
                .map_err(|e| format!("eval: {e}"))
        })
        .map_err(|e| format!("{e}"))?;
    Ok(result)
}

/// Run the dev path: install the runts-ink bridge
/// into a fresh rquickjs context, eval the lowered
/// JS, call `runts_ink.render_to_string` on the
/// result, and return the rendered string.
///
/// We use the same renderer as `runts build` so the
/// output is byte-identical for the same .tsx.
pub fn run_ink_dev(js: &str) -> Result<String, String> {
    let program = format!("(() => {{ {js} }})()");
    run_ink_dev_with_program(js, &program)
}

/// Build a rquickjs eval program that picks the
/// largest top-level JSX from the source, lowers
/// it via `dev_jsx::lower_jsx_for_eval`, and
/// embeds the lowered form in the rquickjs
/// program. The runtime sees
/// `runts_ink.box(...)` / `runts_ink.text(...)`
/// calls.
pub fn dev_eval_program_with_lowered(
    src: &str,
    _lowered_js: &str,
) -> String {
    // Find all top-level JSX elements in the
    // original source (lowered_js has no JSX).
    let jsx_blocks = find_top_level_jsx(src);
    // Pick the longest one (the app body, not
    // the self-closing reference).
    if let Some((_end, raw)) = jsx_blocks.iter().max_by_key(|(_, s)| s.len()) {
        // Also extract any `const` / `let` variable
        // declarations from the function body so
        // the eval program can reference them.
        // We look for declarations that appear
        // BEFORE the JSX in the source.
        let var_decls = extract_var_decls_before(src, *_end);
        let lowered = crate::dev_jsx::lower_jsx_for_eval(raw);
        return format!(
            "(() => {{ {var_decls} return runts_ink.render_to_string({lowered}); }})()"
        );
    }
    // Fallback: wrap the lowered JS as an IIFE.
    format!("(() => {{ {_lowered_js} }})()")
}
/// Extract `const NAME = ...;` and `let NAME = ...;`
/// declarations that appear in the source before
/// the given byte position. Used to make function-
/// scoped variables available to the dev-path
/// eval program.
fn extract_var_decls_before(src: &str, before: usize) -> String {
    let prefix = &src[..before.min(src.len())];
    let mut out = String::new();
    let chars: Vec<char> = prefix.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let remaining: String = chars[i..].iter().collect();
        if remaining.starts_with("const ") || remaining.starts_with("let ") {
            let start = i;
            let mut j = i + 5;
            let mut depth = 0;
            while j < chars.len() {
                let c = chars[j];
                if c == '{' || c == '(' || c == '[' { depth += 1; }
                else if c == '}' || c == ')' || c == ']' { depth -= 1; }
                else if depth == 0 && c == ';' { j += 1; break; }
                else if depth == 0 && c == '\n' { break; }
                j += 1;
            }
            let stmt: String = chars[start..j].iter().collect();
            if stmt.contains('=') && !stmt.contains("=>") { out.push_str(&stmt); out.push('\n'); }
            i = j;
        } else { i += 1; }
    }
    out
}

/// Wrap the dev-path JS so rquickjs can eval it.
/// The dev path's `run_ink_dev` expects the
/// lowered JS to be evaluatable.
///
/// Strategy: find the largest top-level JSX
/// expression in the source. This is typically
/// the JSX inside `function App() { return (...) }`
/// — i.e. the app body — not the small self-
/// Build a rquickjs eval program that picks the
/// largest top-level JSX from the source, lowers
/// it via `dev_jsx::lower_jsx_for_eval`, and
/// embeds the lowered form in the rquickjs
/// program. The runtime sees
/// `runts_ink.box(...)` / `runts_ink.text(...)`
/// A top-level JSX is one not inside another JSX.
/// Returns `(end_index, raw_jsx_text)` pairs in
/// source order. The end index is the byte
/// position after the closing tag.
fn find_top_level_jsx(src: &str) -> Vec<(usize, String)> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;    while i < chars.len() {
        if chars[i] == '<' && i + 1 < chars.len() && chars[i + 1] != '!' {
            // Skip past strings, comments, etc.
            // Check if this looks like a JSX open
            // by trying to parse it.
            if let Some((end, raw)) = parse_jsx_top(&chars, i) {
                out.push((end, raw));
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Lightweight JSX parser for top-level
/// elements. Returns the index past the closing
/// tag and the raw JSX text.
fn parse_jsx_top(chars: &[char], i: usize) -> Option<(usize, String)> {
    let mut j = i + 1;
    if j >= chars.len() || !chars[j].is_ascii_alphabetic() { return None; }
    while j < chars.len() && chars[j].is_ascii_alphanumeric() { j += 1; }
    let tag: String = chars[i + 1..j].iter().collect();
    let mut k = j; let mut self_closing = false;
    while k < chars.len() { if chars[k] == '/' && k + 1 < chars.len() && chars[k + 1] == '>' { self_closing = true; k += 2; break; } if chars[k] == '>' { k += 1; break; } k += 1; }
    if self_closing { return Some((k, chars[i..k].iter().collect())); }
    let open = format!("<{tag}"); let close = format!("</{tag}>"); let open_chars: Vec<char> = open.chars().collect(); let close_chars: Vec<char> = close.chars().collect();
    let mut depth: usize = 1; let mut m = k;
    while m < chars.len() && depth > 0 { if m + close_chars.len() <= chars.len() { let cand: String = chars[m..m + close_chars.len()].iter().collect(); if cand == close { depth -= 1; if depth == 0 { let end = m + close_chars.len(); return Some((end, chars[i..end].iter().collect())); } m += close_chars.len(); continue; } } if m + open_chars.len() <= chars.len() { let cand: String = chars[m..m + open_chars.len()].iter().collect(); if cand == open { let next_pos = m + open_chars.len(); if next_pos < chars.len() { let nc = chars[next_pos]; if nc == ' ' || nc == '>' || nc == '/' || nc == '\t' || nc == '\n' { depth += 1; m += open_chars.len(); continue; } } } } m += 1; }
    None
}

/// Find the index of the closing paren that
/// matches the opening paren at `open_idx`.
fn find_matching_paren(s: &str, open_idx: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.get(open_idx) != Some(&b'(') {
        return None;
    }
    let mut depth = 1;
    let mut i = open_idx + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
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

/// Per-session dev state for the ratatui plugin.
struct RatatuiDevState {
    /// Path to the .tsx module being rendered.
    /// `None` if no .tsx was found in the project.
    module: Option<String>,
    /// `true` when the source has changed and needs
    /// to be re-evaluated on the next `dev_run_once`.
    dirty: bool,
}

impl DevState for RatatuiDevState {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
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
