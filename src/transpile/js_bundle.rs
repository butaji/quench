//! TSX → JS transpiler for the rquickjs dev path.
//!
//! Uses oxc to parse TSX, strip TypeScript types, transform JSX to
//! `React.createElement`, and emit plain JS.  Post-processing rewrites
//! `import { … } from 'ink'` to string-tag constants and injects a
//! minimal React shim so the bundle evals cleanly in rquickjs.

use anyhow::{Context, Result};
use oxc_allocator::Allocator;
use oxc_codegen::Codegen;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{JsxRuntime, Transformer};

/// Transpile a TSX source string into a runnable JS bundle.
///
/// The output:
/// - has JSX desugared to `React.createElement`,
/// - has TypeScript annotations erased,
/// - rewrites Ink imports to string-tag constants,
/// - strips ES module `import` / `export` syntax,
/// - injects a React shim that bridges to `runts_ink`.
pub fn transpile_to_js(source: &str) -> Result<String> {
    let allocator = Allocator::default();
    let program = parse_tsx(&allocator, source)?;
    let mut program = program;
    let js = transform_and_codegen(&allocator, &mut program)?;
    let js = postprocess_bundle(&js);
    Ok(js)
}

fn parse_tsx<'a>(allocator: &'a Allocator, source: &'a str) -> Result<oxc_ast::ast::Program<'a>> {
    let source_type = SourceType::default()
        .with_module(true)
        .with_typescript(true)
        .with_jsx(true);
    let ret = Parser::new(allocator, source, source_type).parse();
    if !ret.errors.is_empty() {
        let msgs: Vec<String> = ret.errors.iter().map(|e| format!("{:?}", e)).collect();
        anyhow::bail!("Parse errors:\n{}", msgs.join("\n"));
    }
    Ok(ret.program)
}

fn transform_and_codegen<'a>(
    allocator: &'a Allocator,
    program: &mut oxc_ast::ast::Program<'a>,
) -> Result<String> {
    let semantic = SemanticBuilder::new().build(program);
    let scoping = semantic.semantic.into_scoping();

    let mut options = oxc_transformer::TransformOptions::default();
    options.jsx.runtime = JsxRuntime::Classic;
    options.jsx.jsx_plugin = true;

    let _ = Transformer::new(allocator, std::path::Path::new("app.tsx"), &options)
        .build_with_scoping(scoping, program);

    let codegen = Codegen::new();
    let output = codegen.build(program);
    Ok(output.code)
}

fn postprocess_bundle(js: &str) -> String {
    let (js, default_name) = strip_exports_and_capture_default(js);
    let js = rewrite_ink_imports(&js);
    let js = strip_react_import(&js);
    let js = strip_remaining_imports(&js);
    let mut out = String::with_capacity(js.len() + REACT_SHIM.len() + 64);
    out.push_str(REACT_SHIM);
    out.push('\n');
    out.push_str(&js);
    if let Some(name) = default_name {
        out.push_str("\nvar __runts_default = ");
        out.push_str(&name);
        out.push(';');
    }
    out
}

fn strip_exports_and_capture_default(js: &str) -> (String, Option<String>) {
    let mut default_name: Option<String> = None;
    let mut out = String::with_capacity(js.len());

    for line in js.lines() {
        let (processed, captured) = process_export_line(line);
        if let Some(name) = captured {
            default_name = Some(name);
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

fn capture_default_function(line: &str) -> Option<String> {
    let rest = line.strip_prefix("export default function")?;
    let name = rest.trim().split(|c: char| c == '(' || c == ' ' || c == '<').next()?;
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

fn capture_default_const(line: &str) -> Option<String> {
    let rest = line.strip_prefix("export default const")?;
    let name = rest.trim().split(|c: char| c == '=' || c == ' ' || c == ':').next()?;
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

fn capture_default_expr(line: &str) -> Option<String> {
    let rest = line.strip_prefix("export default")?;
    let rest = rest.trim();
    if !rest.ends_with(';') {
        return None;
    }
    let name = rest[..rest.len() - 1].trim();
    if name.is_empty() {
        return None;
    }
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

fn ink_import_to_const(spec: &str) -> String {
    if let Some(pos) = spec.find(" as ") {
        let orig = spec[..pos].trim();
        let alias = spec[pos + 4..].trim();
        format!(r#"const {} = "{}";"#, alias, orig)
    } else {
        format!(r#"const {} = "{}";"#, spec, spec)
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

const REACT_SHIM: &str = r#"var React = {
    createElement: function(type, props, ...children) {
        props = props || {};
        if (children.length > 0) {
            props.children = children;
        }
        if (typeof type === 'function') {
            return type(props);
        }
        if (type === 'Box') return runts_ink.box(props);
        if (type === 'Text') {
            var content = children.length > 0 ? String(children[0]) : '';
            delete props.children;
            return runts_ink.text(content, props);
        }
        if (type === 'Newline') return runts_ink.newline();
        if (type === 'Spacer') return runts_ink.spacer();
        return runts_ink.box(props);
    }
};"#;

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    #[test]
    fn transpile_basic_tsx() {
        let source = r#"
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return <Box><Text bold>hi</Text></Box>;
}
"#;
        let js = transpile_to_js(source).unwrap();
        assert!(js.contains("function App()"));
        assert!(js.contains("React.createElement"));
        assert!(js.contains(r#"const Box = "Box";"#));
        assert!(js.contains(r#"const Text = "Text";"#));
        assert!(js.contains("var __runts_default = App;"));
        assert!(!js.contains("import "));
    }

    #[test]
    fn transpile_strips_types() {
        let source = r#"
import React from 'react';
import { Box } from 'ink';

interface Props { name: string; }

export default function App(props: Props) {
  const x: number = 42;
  return <Box>{props.name}</Box>;
}
"#;
        let js = transpile_to_js(source).unwrap();
        assert!(!js.contains("interface Props"));
        assert!(!js.contains("props: Props"));
        assert!(js.contains("const x = 42;"));
    }

    #[test]
    fn transpile_ink_text_props_evals() {
        let path = std::path::Path::new("examples/ink-text-props/tui/app.tsx");
        if !path.exists() {
            eprintln!("Skipping test: {} not found", path.display());
            return;
        }
        let source = std::fs::read_to_string(path).unwrap();
        let js = transpile_to_js(&source).unwrap();

        let runtime = Runtime::new().unwrap();
        let ctx = Context::full(&runtime).unwrap();
        ctx.with(|ctx| {
            runts_ink::js_bridge::install(&ctx).unwrap();
            let result: Result<rquickjs::Value, _> = ctx.eval(js.as_str());
            assert!(result.is_ok(), "Eval failed: {:?}", result.err());

            let default_export: rquickjs::Value = ctx.globals().get("__runts_default").unwrap();
            assert!(
                default_export.is_function(),
                "__runts_default should be a function"
            );
        });
    }
}
