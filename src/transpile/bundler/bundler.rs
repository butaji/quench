//! Bundler core implementation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{JsxRuntime, Transformer};

use crate::transpile::bundler::imports;
use crate::transpile::bundler::transform;

pub struct Bundler {
    allocator: Allocator,
    pub modules: Vec<ModuleData>,
    pub module_index: HashMap<PathBuf, usize>,
}

pub struct ModuleData {
    pub path: PathBuf,
    pub exports: HashMap<String, String>,
    pub js: String,
}

impl Bundler {
    pub fn new() -> Self {
        Self {
            allocator: Allocator::default(),
            modules: Vec::new(),
            module_index: HashMap::new(),
        }
    }

    pub fn resolve_modules(&mut self, file_path: &Path, from_dir: &Path) -> Result<()> {
        let source = std::fs::read_to_string(file_path)?;
        let canonical = file_path.canonicalize().unwrap_or_else(|_| file_path.to_path_buf());
        if self.module_index.contains_key(&canonical) { return Ok(()); }

        self.modules.push(ModuleData {
            path: canonical.clone(),
            exports: HashMap::new(),
            js: String::new(),
        });
        let id = self.modules.len() - 1;
        self.module_index.insert(canonical.clone(), id);

        for import_path in imports::find_imports(&source) {
            if import_path.starts_with('.') {
                if let Some(resolved) = self.resolve_import(&import_path, from_dir) {
                    self.resolve_modules(&resolved, resolved.parent().unwrap_or(from_dir))?;
                }
            }
        }
        Ok(())
    }

    pub fn transpile_modules(&mut self, file_path: &Path) -> Result<()> {
        let source = std::fs::read_to_string(file_path)?;
        let program = parse_tsx(&self.allocator, &source)?;
        let mut program = program;
        let js = transform_and_codegen(&self.allocator, &mut program)?;

        let (raw_js, exports) = self.extract_exports(&js)?;
        let canonical = file_path.canonicalize().unwrap_or_else(|_| file_path.to_path_buf());
        let id = self.module_index.get(&canonical).copied().unwrap_or(0);

        let js = transform::rename_module_declarations(&raw_js, id);
        let updated_exports = transform::rename_default_export(&exports, id);

        if let Some(module) = self.modules.get_mut(id) {
            module.exports = updated_exports;
            module.js = js;
        }
        Ok(())
    }

    fn extract_exports(&self, js: &str) -> Result<(String, HashMap<String, String>)> {
        let mut exports = HashMap::new();
        let mut output_lines = Vec::new();

        for line in js.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("import ") {
                output_lines.push(line.to_string());
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

    pub fn rewrite_imports(&mut self) {
        let rewritten: Vec<(usize, String)> = self.modules.iter()
            .enumerate()
            .map(|(i, m)| {
                let from_dir = m.path.parent().unwrap_or(Path::new("."));
                let js = self.rewrite_module_imports(&m.js, from_dir);
                (i, js)
            })
            .collect();
        for (i, js) in rewritten {
            self.modules[i].js = js;
        }
    }

    fn rewrite_module_imports(&self, js: &str, from_dir: &Path) -> String {
        let mut output_lines = Vec::new();
        for line in js.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("import ") {
                if let Some(rewritten) = self.rewrite_import_line(trimmed, from_dir) {
                    output_lines.extend(rewritten);
                    continue;
                }
            }
            output_lines.push(line.to_string());
        }
        output_lines.join("\n")
    }

    fn rewrite_import_line(&self, line: &str, from_dir: &Path) -> Option<Vec<String>> {
        let trimmed = line.trim();
        if imports::is_react_import(trimmed) { return Some(Vec::new()); }
        if imports::is_ink_import(trimmed) { return Some(imports::extract_ink_import_declarations(trimmed)); }
        if !imports::is_local_import(trimmed) { return None; }

        if let Ok(Some((_, resolved, names))) = self.resolve_local_import(trimmed, from_dir) {
            let id = self.get_module_id(&resolved);
            let all_exports = self.modules.iter()
                .find(|m| m.path == resolved.canonicalize().unwrap_or(resolved.clone()))
                .map(|m| m.exports.clone())
                .unwrap_or_default();
            return Some(vec![transform::rewrite_import_to_global(id, &names, &all_exports)]);
        }
        None
    }

    fn process_export_line(&self, line: &str) -> Option<(String, String, String)> {
        let trimmed = line.trim();
        if let Some(name) = crate::transpile::postprocess::capture_default_function(trimmed) {
            let transformed = line.replacen("export default function", "function __mD", 1);
            return Some(("default".to_string(), name, transformed));
        }
        if let Some(name) = crate::transpile::postprocess::capture_default_const(trimmed) {
            let transformed = line.replacen("export default const", "const __mD", 1);
            return Some(("default".to_string(), name, transformed));
        }
        if let Some(name) = crate::transpile::postprocess::capture_default_identifier(trimmed) {
            return Some(("default".to_string(), name, "// export default handled".to_string()));
        }
        imports::extract_named_export(trimmed).map(|(k, n)| (k, n, line.to_string()))
    }

    fn resolve_local_import(&self, line: &str, from_dir: &Path) -> Result<Option<(String, PathBuf, Vec<String>)>> {
        let trimmed = line.trim();
        if !trimmed.starts_with("import") { return Ok(None); }

        let re = regex::Regex::new(r#"import\s+(\{[^}]+\}|\*\s+as\s+\w+|\w+)\s+from\s+['"]([^'"]+)['"]"#)?;
        if let Some(caps) = re.captures(trimmed) {
            let import_spec = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let from_path = caps.get(2).map(|m| m.as_str()).unwrap_or("");

            if from_path.starts_with('.') {
                if let Some(resolved) = self.resolve_import(from_path, from_dir) {
                    return Ok(Some((trimmed.to_string(), resolved, imports::parse_import_names(import_spec))));
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

pub fn parse_tsx<'a>(allocator: &'a Allocator, source: &'a str) -> Result<oxc_ast::ast::Program<'a>> {
    let source_type = SourceType::default().with_module(true).with_typescript(true).with_jsx(true);
    let ret = Parser::new(allocator, source, source_type).parse();
    if !ret.errors.is_empty() {
        let msgs: Vec<String> = ret.errors.iter().map(|e| format!("{:?}", e)).collect();
        anyhow::bail!("Parse errors:\n{}", msgs.join("\n"));
    }
    Ok(ret.program)
}

pub fn transform_and_codegen<'a>(allocator: &'a Allocator, program: &mut oxc_ast::ast::Program<'a>) -> Result<String> {
    let semantic = SemanticBuilder::new().build(program);
    let scoping = semantic.semantic.into_scoping();

    let mut options = oxc_transformer::TransformOptions::default();
    options.jsx.runtime = JsxRuntime::Classic;
    options.jsx.jsx_plugin = true;

    let _ = Transformer::new(allocator, std::path::Path::new("app.tsx"), &options)
        .build_with_scoping(scoping, program);

    Ok(oxc_codegen::Codegen::new().build(program).code)
}
