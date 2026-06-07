//! Post-processing for transpiled JS bundles.
//!
//! Handles export stripping, import rewriting, and React shim injection.

use crate::transpile::js_bundle::react_shim::{POST_SHIM, REACT_SHIM};

/// Transpile a TSX source string into a runnable JS bundle.
pub fn transpile_to_js(source: &str) -> anyhow::Result<String> {
    let allocator = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::default()
        .with_module(true)
        .with_typescript(true)
        .with_jsx(true);

    let ret = oxc_parser::Parser::new(&allocator, source, source_type).parse();
    if !ret.errors.is_empty() {
        let msgs: Vec<String> = ret.errors.iter().map(|e| format!("{:?}", e)).collect();
        anyhow::bail!("Parse errors:\n{}", msgs.join("\n"));
    }

    let semantic = oxc_semantic::SemanticBuilder::new().build(&ret.program);
    let scoping = semantic.semantic.into_scoping();

    let mut options = oxc_transformer::TransformOptions::default();
    options.jsx.runtime = oxc_transformer::JsxRuntime::Classic;
    options.jsx.jsx_plugin = true;

    let mut program = ret.program;
    let _ = oxc_transformer::Transformer::new(&allocator, std::path::Path::new("app.tsx"), &options)
        .build_with_scoping(scoping, &mut program);

    let js = oxc_codegen::Codegen::new().build(&program).code;
    let js = postprocess_bundle(&js);
    Ok(js)
}

fn postprocess_bundle(js: &str) -> String {
    let (js, default_name) = strip_exports_and_capture_default(js);
    let js = rewrite_ink_imports(&js);
    let js = strip_react_import(&js);
    let js = strip_remaining_imports(&js);

    let mut out = String::with_capacity(js.len() + REACT_SHIM.len() + 256);
    out.push_str(REACT_SHIM);
    out.push('\n');
    out.push_str(&js);

    if let Some(name) = default_name {
        out.push_str("\nvar __runts_default = React._withHooks(");
        out.push_str(&name);
        out.push_str(");");
    }

    out.push_str(POST_SHIM);
    out
}

fn strip_exports_and_capture_default(js: &str) -> (String, Option<String>) {
    let mut default_name: Option<String> = None;
    let mut out = String::with_capacity(js.len());

    for line in js.lines() {
        let (processed, captured) = process_export_line(line);
        if captured.is_some() {
            default_name = captured;
        }
        out.push_str(&processed);
    }

    (out, default_name)
}

fn process_export_line(line: &str) -> (String, Option<String>) {
    let trimmed = line.trim();

    if let Some(name) = capture_default_function(trimmed) {
        return (line.replacen("export default function", "function", 1) + "\n", Some(name));
    }
    if let Some(name) = capture_default_const(trimmed) {
        return (line.replacen("export default const", "const", 1) + "\n", Some(name));
    }
    if let Some(name) = capture_default_expr(trimmed) {
        return (String::new(), Some(name));
    }
    if trimmed.starts_with("export function ") {
        return (line.replacen("export function", "function", 1) + "\n", None);
    }
    if trimmed.starts_with("export const ") {
        return (line.replacen("export const", "const", 1) + "\n", None);
    }

    (line.to_string() + "\n", None)
}

pub fn capture_default_function(line: &str) -> Option<String> {
    let rest = line.strip_prefix("export default function")?;
    let name = rest.trim().split(|c: char| c == '(' || c == ' ' || c == '<').next()?;
    if name.is_empty() { return None; }
    Some(name.to_string())
}

pub fn capture_default_const(line: &str) -> Option<String> {
    let rest = line.strip_prefix("export default const")?;
    let name = rest.trim().split(|c: char| c == '=' || c == ' ' || c == ':').next()?;
    if name.is_empty() { return None; }
    Some(name.to_string())
}

fn capture_default_expr(line: &str) -> Option<String> {
    let rest = line.strip_prefix("export default")?;
    let rest = rest.trim();
    if !rest.ends_with(';') { return None; }
    let name = rest[..rest.len() - 1].trim();
    if name.is_empty() { return None; }
    Some(name.to_string())
}

fn rewrite_ink_imports(js: &str) -> String {
    let re = regex::Regex::new(
        r#"(?m)^\s*import\s+\{\s*([^}]+)\s*\}\s+from\s+['"]ink['"]\s*;?\s*$"#,
    )
    .expect("valid regex");

    re.replace_all(js, |caps: &regex::Captures| {
        let names: Vec<&str> = caps[1].split(',').map(|s| s.trim()).collect();
        let decls: Vec<String> = names
            .iter()
            .filter(|n| !n.is_empty())
            .map(|n| ink_import_to_const(n))
            .collect();

        if decls.is_empty() {
            String::new()
        } else {
            decls.join("\n") + "\n"
        }
    })
    .to_string()
}

static INK_HOOKS: &[&str] = &[
    "useInput", "useApp", "useStdin", "useStdout", "useStderr",
    "useWindowSize", "useFocus", "useFocusManager", "useCursor",
    "useAnimation", "usePaste", "render",
];

fn ink_import_to_const(spec: &str) -> String {
    let (orig, alias) = if let Some(pos) = spec.find(" as ") {
        (spec[..pos].trim(), spec[pos + 4..].trim())
    } else {
        (spec, spec)
    };

    if INK_HOOKS.contains(&orig) {
        format!(r#"const {} = runts_ink_hooks.{};"#, alias, orig)
    } else {
        format!(r#"const {} = "{}";"#, alias, orig)
    }
}

fn strip_react_import(js: &str) -> String {
    let re = regex::Regex::new(
        r#"(?m)^\s*import\s+React\s+from\s+['"]react['"]\s*;?\s*$"#,
    )
    .expect("valid regex");
    re.replace_all(js, "").to_string()
}

fn strip_remaining_imports(js: &str) -> String {
    let re = regex::Regex::new(r"(?m)^\s*import\s+.*?;\s*$").expect("valid regex");
    re.replace_all(js, "").to_string()
}
