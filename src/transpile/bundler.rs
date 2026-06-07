//! Module bundler for TSX → JS transpilation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{JsxRuntime, Transformer};

use crate::transpile::js_bundle::react_shim::{POST_SHIM, REACT_SHIM};
use crate::transpile::postprocess;

struct Bundler {
    allocator: Allocator,
    modules: Vec<ModuleData>,
    module_index: HashMap<PathBuf, usize>,
}

struct ModuleData {
    path: PathBuf,
    exports: HashMap<String, String>,
    js: String,
}

impl Bundler {
    fn new() -> Self {
        Self {
            allocator: Allocator::default(),
            modules: Vec::new(),
            module_index: HashMap::new(),
        }
    }

    fn resolve_modules(&mut self, file_path: &Path, from_dir: &Path) -> Result<()> {
        let source = std::fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read {}", file_path.display()))?;

        let canonical = file_path.canonicalize().unwrap_or_else(|_| file_path.to_path_buf());
        if self.module_index.contains_key(&canonical) {
            return Ok(());
        }

        self.modules.push(ModuleData {
            path: canonical.clone(),
            exports: HashMap::new(),
            js: String::new(),
        });
        let id = self.modules.len() - 1;
        self.module_index.insert(canonical.clone(), id);

        let imports = find_imports(&source);
        for import_path in imports {
            if import_path.starts_with('.') {
                if let Some(resolved) = self.resolve_import(&import_path, from_dir) {
                    self.resolve_modules(&resolved, resolved.parent().unwrap_or(from_dir))?;
                }
            }
        }

        Ok(())
    }

    fn transpile_modules(&mut self, file_path: &Path) -> Result<()> {
        let source = std::fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read {}", file_path.display()))?;
        let program = parse_tsx(&self.allocator, &source)?;
        let mut program = program;
        let js = transform_and_codegen(&self.allocator, &mut program)?;

        let (js, exports) = self.process_module(&js, file_path.parent().unwrap_or(Path::new(".")))?;
        let canonical = file_path.canonicalize().unwrap_or_else(|_| file_path.to_path_buf());
        let id = self.module_index.get(&canonical).copied().unwrap_or(0);

        let js = rename_module_declarations(&js, id);
        let updated_exports = rename_default_export(&exports, id);

        if let Some(module) = self.modules.get_mut(id) {
            module.exports = updated_exports;
            module.js = js;
        }

        Ok(())
    }

    fn process_module(&self, js: &str, from_dir: &Path) -> Result<(String, HashMap<String, String>)> {
        let mut exports = HashMap::new();
        let mut output_lines = Vec::new();

        for line in js.lines() {
            let trimmed = line.trim();

            if let Some(processed) = self.process_import_line(trimmed, from_dir) {
                output_lines.extend(processed);
                continue;
            }

            if let Some((kind, name, transformed)) = self.process_export_line(trimmed) {
                exports.insert(kind, name);
                output_lines.push(transformed);
                continue;
            }

            output_lines.push(line.to_string());
        }

        Ok((output_lines.join("\n"), exports))
    }

    fn process_import_line(&self, line: &str, from_dir: &Path) -> Option<Vec<String>> {
        let trimmed = line.trim();
        let decls = match handle_import_line(trimmed) {
            Some(d) => d,
            None => {
                if is_local_import(trimmed) {
                    return self.resolve_local_import_to_global(trimmed, from_dir);
                }
                return None;
            }
        };
        if decls.is_empty() && is_local_import(trimmed) {
            return self.resolve_local_import_to_global(trimmed, from_dir);
        }
        Some(decls)
    }

    fn resolve_local_import_to_global(&self, line: &str, from_dir: &Path) -> Option<Vec<String>> {
        if let Ok(Some((_, resolved, names))) = self.resolve_local_import(line, from_dir) {
            let id = self.get_module_id(&resolved);
            return Some(vec![rewrite_import_to_global(id, &names)]);
        }
        None
    }

    fn process_export_line(&self, line: &str) -> Option<(String, String, String)> {
        let trimmed = line.trim();
        if let Some(name) = postprocess::capture_default_function(trimmed) {
            let transformed = line.replacen("export default function", "function __mD", 1);
            return Some(("default".to_string(), name, transformed));
        }
        if let Some(name) = postprocess::capture_default_const(trimmed) {
            let transformed = line.replacen("export default const", "const __mD", 1);
            return Some(("default".to_string(), name, transformed));
        }
        if let Some(name) = extract_named_export(trimmed) {
            return Some((name.0, name.1, line.to_string()));
        }
        None
    }

    fn resolve_local_import(&self, line: &str, from_dir: &Path) -> Result<Option<(String, PathBuf, Vec<String>)>> {
        let trimmed = line.trim();
        if !trimmed.starts_with("import") {
            return Ok(None);
        }

        let re = regex::Regex::new(r#"import\s+(\{[^}]+\}|\*\s+as\s+\w+|\w+)\s+from\s+['"]([^'"]+)['"]"#)?;
        if let Some(caps) = re.captures(trimmed) {
            let import_spec = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let from_path = caps.get(2).map(|m| m.as_str()).unwrap_or("");

            if from_path.starts_with('.') {
                if let Some(resolved) = self.resolve_import(from_path, from_dir) {
                    return Ok(Some((trimmed.to_string(), resolved, parse_import_names(import_spec))));
                }
            }
        }

        Ok(None)
    }

    fn resolve_import(&self, import_path: &str, from_dir: &Path) -> Option<PathBuf> {
        let base = from_dir.join(import_path);
        if base.exists() { return Some(base); }

        for ext in &["tsx", "ts", "jsx", "js"] {
            let with_ext = base.with_extension(*ext);
            if with_ext.exists() { return Some(with_ext); }
        }

        for index_name in &["index.tsx", "index.ts", "index.js"] {
            let index = base.join(index_name);
            if index.exists() { return Some(index); }
        }

        None
    }

    fn get_module_id(&self, path: &Path) -> usize {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.module_index.get(&canonical).copied().unwrap_or(0)
    }
}

fn rename_module_declarations(js: &str, id: usize) -> String {
    let prefix = format!("__m{}", id);
    // Replace __mD (from process_module) with module-prefixed names
    // For id=0: __mD TextProps -> __m0_TextProps
    let mut result = js.replace("function __mD ", &format!("function {}_", prefix));
    result = result.replace("const __mD ", &format!("const {}_", prefix));
    // Prefix other declarations (var/let/const without __m prefix)
    result = prefix_declarations(&result, &prefix);
    result
}

fn rename_default_export(exports: &HashMap<String, String>, id: usize) -> HashMap<String, String> {
    exports.iter()
        .map(|(k, v)| {
            if k == "default" && !v.is_empty() {
                (k.clone(), format!("__m{}_{}", id, v))
            } else {
                (k.clone(), v.clone())
            }
        })
        .collect()
}

fn find_imports(js: &str) -> Vec<String> {
    let re = regex::Regex::new(r#"import\s+.*?\s+from\s+['"]([^'"]+)['"]"#).unwrap();
    re.captures_iter(js)
        .filter_map(|cap| {
            let path = cap.get(1)?.as_str().to_string();
            if path.starts_with('.') { Some(path) } else { None }
        })
        .collect()
}

fn parse_import_names(spec: &str) -> Vec<String> {
    let spec = spec.trim();
    if spec.starts_with('{') && spec.ends_with('}') {
        spec[1..spec.len()-1]
            .split(',')
            .map(|s| s.trim().split(" as ").next().unwrap_or(s.trim()).to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else if spec.starts_with("* as ") {
        vec![spec.trim_start_matches("* as ").to_string()]
    } else {
        vec![spec.to_string()]
    }
}

fn rewrite_import_to_global(module_id: usize, names: &[String]) -> String {
    names.iter()
        .map(|name| format!("var {} = __m{}_{};", name, module_id, name))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_local_import(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with("import ") {
        return false;
    }
    // Check for double-quoted local imports
    if let Some(from_pos) = trimmed.find("from \"") {
        let path_start = from_pos + 6;
        let rest = &trimmed[path_start..];
        if let Some(end) = rest.find('\"') {
            let path = &rest[..end];
            return is_relative_path(path);
        }
    }
    // Check for single-quoted local imports
    if let Some(from_pos) = trimmed.find("from '") {
        let path_start = from_pos + 6;
        let rest = &trimmed[path_start..];
        if let Some(end) = rest.find('\'') {
            let path = &rest[..end];
            return is_relative_path(path);
        }
    }
    false
}

fn is_relative_path(path: &str) -> bool {
    let starts_dot_slash = path.starts_with("./");
    let starts_dot_dot = path.starts_with("../");
    starts_dot_slash || starts_dot_dot
}

fn handle_import_line(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if !trimmed.starts_with("import ") {
        return None;
    }
    // React import is handled by shim
    if is_react_import(trimmed) {
        return Some(Vec::new());
    }
    // Ink imports become string constants
    if is_ink_import(trimmed) {
        return Some(extract_ink_import_declarations(trimmed));
    }
    // Other imports (npm packages, etc.) are skipped
    Some(Vec::new())
}

fn is_react_import(line: &str) -> bool {
    line.starts_with("import React from") || line.starts_with("import React, ")
}

fn is_ink_import(line: &str) -> bool {
    if !line.starts_with("import { ") {
        return false;
    }
    line.contains("from 'ink'") || line.contains("from \"ink\"")
}

// Ink hooks that should reference runts_ink_hooks, not be string constants
const INK_HOOKS: &[&str] = &[
    "useInput", "useApp", "useStdin", "useStdout", "useStderr",
    "useWindowSize", "useFocus", "useFocusManager", "useCursor",
    "useAnimation", "usePaste", "useBoxMetrics", "useRef",
    "render", "measureElement",
];

fn ink_import_to_const(spec: &str) -> String {
    let (orig, alias) = if let Some(pos) = spec.find(" as ") {
        (spec[..pos].trim(), spec[pos + 4..].trim())
    } else {
        (spec, spec)
    };

    if INK_HOOKS.contains(&orig) {
        format!("var {} = runts_ink_hooks.{};", alias, orig)
    } else {
        format!("var {} = '{}';", alias, orig)
    }
}

fn extract_ink_import_declarations(line: &str) -> Vec<String> {
    let re = regex::Regex::new(r#"import\s+\{\s*([^}]+)\s*\}\s+from\s+['"]ink['"]"#).unwrap();
    if let Some(caps) = re.captures(line) {
        caps.get(1)
            .map(|m| m.as_str())
            .map(|spec| {
                spec.split(',')
                    .map(|s| ink_import_to_const(s.trim()))
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    }
}

fn extract_named_export(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if let Some(name) = extract_const_name(trimmed) {
        return Some((name.clone(), name));
    }
    extract_function_name(trimmed).map(|n| (n.clone(), n))
}

fn extract_const_name(line: &str) -> Option<String> {
    if !line.starts_with("export const ") { return None; }
    let after = line.strip_prefix("export const ")?
        .split(|c: char| c == '=' || c == ':' || c == ' ').next()?;
    if after.is_empty() { None } else { Some(after.into()) }
}

fn extract_function_name(line: &str) -> Option<String> {
    if !line.starts_with("export function ") { return None; }
    let after = line.strip_prefix("export function ")?
        .split(|c: char| c == '(' || c == ' ' || c == '<').next()?;
    if after.is_empty() { None } else { Some(after.into()) }
}

fn prefix_declarations(js: &str, prefix: &str) -> String {
    let mut output = String::new();
    let mut brace_depth = 0;
    for line in js.lines() {
        let trimmed = line.trim();
        // Track brace depth to know if we're inside a function
        brace_depth += trimmed.matches('{').count() as i32;
        brace_depth -= trimmed.matches('}').count() as i32;
        // Only prefix var declarations at module level (brace_depth == 0)
        // Skip const/let as they define module-level variables used in JSX
        if brace_depth == 0 {
            output.push_str(&prefix_var_at_module_level(line, prefix));
        } else {
            output.push_str(line);
        }
        output.push('\n');
    }
    output
}

fn prefix_var_at_module_level(line: &str, prefix: &str) -> String {
    let trimmed = line.trim();
    // Only handle var declarations at module level (not const/let)
    if trimmed.starts_with("var ") {
        if let Some(prefixed) = prefix_var_decl(trimmed, "var ", prefix) {
            return line.replacen("var ", &prefixed.replacen("var ", "var ", 1), 1);
        }
    }
    line.to_string()
}

fn prefix_var_decl(trimmed: &str, kw: &str, prefix: &str) -> Option<String> {
    if !trimmed.starts_with(kw) { return None; }
    let after_kw = trimmed.strip_prefix(kw)?;
    // Check if this is a simple variable name (not a pattern like [a, b] or {a, b})
    let first_char = after_kw.chars().next()?;
    if first_char == '[' || first_char == '{' {
        // This is a destructuring pattern - don't prefix
        return None;
    }
    let name_end = after_kw.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(after_kw.len());
    let name = &after_kw[..name_end];
    // Skip declarations that are already module-prefixed or are ink component tags
    if name.starts_with("__m") || name.starts_with('_') { return None; }
    // Skip ink component declarations (var Box = 'Box') - these are global
    if after_kw[name_end..].starts_with(" = '") || after_kw[name_end..].starts_with(" = \"") {
        return None;
    }
    // Skip hook declarations (var useXxx = runts_ink_hooks.xxx) - these are global
    if after_kw.contains("runts_ink_hooks") {
        return None;
    }
    // Skip React hook calls (const counterRef = useRef(0)) - these are local vars
    if after_kw.contains("= use") || after_kw.contains("=React.") {
        return None;
    }
    // Skip cross-module references (var Xxx = __mN_Yyy) - these are imports
    if after_kw.contains("= __m") {
        return None;
    }
    let rest = &after_kw[name_end..];
    Some(format!("{}{}{}{}", kw, prefix, name, rest))
}

fn parse_tsx<'a>(allocator: &'a Allocator, source: &'a str) -> Result<oxc_ast::ast::Program<'a>> {
    let source_type = SourceType::default().with_module(true).with_typescript(true).with_jsx(true);
    let ret = Parser::new(allocator, source, source_type).parse();
    if !ret.errors.is_empty() {
        let msgs: Vec<String> = ret.errors.iter().map(|e| format!("{:?}", e)).collect();
        anyhow::bail!("Parse errors:\n{}", msgs.join("\n"));
    }
    Ok(ret.program)
}

fn transform_and_codegen<'a>(allocator: &'a Allocator, program: &mut oxc_ast::ast::Program<'a>) -> Result<String> {
    let semantic = SemanticBuilder::new().build(program);
    let scoping = semantic.semantic.into_scoping();

    let mut options = oxc_transformer::TransformOptions::default();
    options.jsx.runtime = JsxRuntime::Classic;
    options.jsx.jsx_plugin = true;

    let _ = Transformer::new(allocator, std::path::Path::new("app.tsx"), &options)
        .build_with_scoping(scoping, program);

    Ok(oxc_codegen::Codegen::new().build(program).code)
}

pub fn transpile_to_js_bundled(entry_path: &Path) -> Result<String> {
    let mut bundler = Bundler::new();
    let from_dir = entry_path.parent().unwrap_or(Path::new("."));

    bundler.resolve_modules(entry_path, from_dir)?;

    let entry_canonical = entry_path.canonicalize().unwrap_or_else(|_| entry_path.to_path_buf());
    let mut ordered: Vec<PathBuf> = bundler.module_index.keys().cloned().collect();
    ordered.sort();

    for path in &ordered {
        bundler.transpile_modules(path)?;
    }

    build_bundle_output(&bundler, &entry_canonical)
}

fn build_bundle_output(bundler: &Bundler, entry_canonical: &Path) -> Result<String> {
    let mut output = REACT_SHIM.to_string();
    output.push_str("\n// Bundled modules\n");

    for module in &bundler.modules {
        output.push_str(&module.js);
        output.push('\n');
    }

    let entry_module = bundler.modules.iter().find(|m| m.path == *entry_canonical);
    if let Some(module) = entry_module {
        if let Some(default_fn) = module.exports.get("default") {
            output.push_str(&format!("\nvar __runts_default = React._withHooks({});", default_fn));
        }
    }

    output.push_str(POST_SHIM);
    Ok(output)
}
