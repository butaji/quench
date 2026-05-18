//! # Core Emitter
//!
//! Main RustEmitter implementation.

use super::ast_walker::AstWalker;
use super::utils::escape_keyword;
use crate::codegen::{CodegenOptions, GeneratedModule};
use crate::parser::swc_parser::SwcAst;
use crate::{analyzer::AnalysisResult, parser::SourceFile};
use std::collections::HashSet;

/// Options for code emission.
#[derive(Debug, Clone, Default)]
pub struct EmitOptions {
    /// Generate debug info
    pub source_map: bool,
    /// Pretty print output
    pub pretty: bool,
    /// Custom imports to add to generated code
    pub custom_imports: Vec<String>,
}

impl EmitOptions {
    /// Create new options with defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a custom import.
    #[must_use]
    pub fn with_import(mut self, import: impl Into<String>) -> Self {
        self.custom_imports.push(import.into());
        self
    }


}

/// Emits Rust code from TypeScript source.
pub struct RustEmitter {
    /// Analysis results
    #[allow(dead_code)]
    analysis: AnalysisResult,
    /// Imports needed by this module (path → names)
    imports: Vec<(String, Vec<String>)>,
    /// Output buffer
    output: String,
    /// Current indentation level
    indent: usize,
    /// Generation options
    #[allow(dead_code)]
    options: CodegenOptions,
    /// Source line mappings for error translation
    source_line_map: Vec<(u32, u32)>,
    /// Emit options for customization
    emit_options: EmitOptions,
}

impl RustEmitter {
    /// Create a new emitter.
    #[allow(clippy::unused_self)]
    pub fn new(_source: &SourceFile, analysis: &AnalysisResult) -> Self {
        let emit_options = EmitOptions::new();
        Self {
            analysis: analysis.clone(),
            imports: Vec::new(),
            output: String::new(),
            indent: 0,
            options: CodegenOptions::default(),
            source_line_map: Vec::new(),
            emit_options,
        }
    }

    /// Set emit options.
    #[must_use]
    pub fn with_options(mut self, options: EmitOptions) -> Self {
        self.emit_options = options;
        self
    }

    /// Emit the complete module using AST walking.
    pub fn emit(mut self, source: &SourceFile) -> crate::Result<GeneratedModule> {
        let (file_stem, module_name) = self.extract_names(source);
        let ast = self.parse_source(source, &file_stem)?;
        let (native_imports, emitted_code) = self.walk_and_emit(&ast)?;

        self.write_header(&module_name, &native_imports);
        self.output.push_str(&emitted_code);

        let output = std::mem::take(&mut self.output);
        let imports = self.convert_imports();

        Ok(GeneratedModule {
            name: file_stem,
            source: output,
            imports,
            types: Vec::new(),
            functions: Vec::new(),
        })
    }

    /// Extract file stem and module name from source.
    fn extract_names(&self, source: &SourceFile) -> (String, String) {
        let file_stem = source
            .path
            .file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("module")
            .to_string();
        let module_name = escape_keyword(&file_stem);
        (file_stem, module_name)
    }

    /// Parse source text to AST.
    fn parse_source(&self, source: &SourceFile, file_stem: &str) -> crate::Result<SwcAst> {
        let source_text = &source.source;
        if source.is_tsx() {
            SwcAst::parse_tsx(source_text, file_stem)
        } else {
            SwcAst::parse_ts(source_text, file_stem)
        }
        .map_err(|e| crate::RunieError::Parse(crate::ParseError::Parse(format!("{e}"))))
    }

    /// Walk AST and emit Rust code.
    fn walk_and_emit(&self, ast: &SwcAst) -> crate::Result<(HashSet<String>, String)> {
        let mut walker = AstWalker::with_analysis(self.analysis.clone());
        walker.walk_module(&ast.module);
        let native_imports = walker.native_imports().clone();
        let output = walker.into_output();
        Ok((native_imports, output))
    }

    /// Convert internal import format to generated module imports.
    fn convert_imports(&self) -> Vec<crate::codegen::Import> {
        self.imports
            .iter()
            .map(|(path, names)| crate::codegen::Import {
                path: path.clone(),
                names: names
                    .iter()
                    .map(|n| crate::codegen::ImportedName {
                        original: n.clone(),
                        rust_name: n.clone(),
                    })
                    .collect(),
                is_native: false,
            })
            .collect()
    }

    /// Register a source line mapping for error translation.
    pub fn register_source_line(&mut self, generated_line: u32, source_line: u32) {
        self.source_line_map.push((generated_line, source_line));
    }

    /// Get source line mappings.
    #[must_use]
    pub fn source_line_map(&self) -> &[(u32, u32)] {
        &self.source_line_map
    }

    /// Write module header with imports.
    fn write_header(&mut self, module_name: &str, native_imports: &HashSet<String>) {
        self.push_line(&format!("// Module: {module_name}"));
        self.push_line("");
        self.write_custom_imports();
        self.write_native_imports(native_imports);
    }

    /// Write custom imports from emit options.
    fn write_custom_imports(&mut self) {
        let imports = self.emit_options.custom_imports.clone();
        for import in &imports {
            self.push_line(import);
        }
        if !imports.is_empty() {
            self.push_line("");
        }
    }

    /// Write native module imports.
    fn write_native_imports(&mut self, native_imports: &HashSet<String>) {
        if !native_imports.is_empty() {
            self.push_line("use crate::native;");
            for module in native_imports {
                self.push_line(&format!("use crate::native::{module};"));
            }
            self.push_line("");
        }
    }

    /// Push a line to output.
    pub fn push_line(&mut self, s: &str) {
        self.output.push_str(s);
        self.output.push('\n');
    }

    /// Push a string to output without newline.
    pub fn push_str(&mut self, s: &str) {
        self.output.push_str(s);
    }

    /// Push indentation.
    pub fn push_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
    }
}
