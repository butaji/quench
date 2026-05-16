//! # Core Emitter
//!
//! Main RustEmitter implementation.

use crate::{parser::SourceFile, analyzer::AnalysisResult};
use crate::codegen::{GeneratedModule, CodegenOptions};
use super::ast_walker::AstWalker;
use crate::parser::swc_parser::SwcAst;

/// Options for code emission.
#[derive(Debug, Clone, Default)]
pub struct EmitOptions {
    /// Generate debug info
    pub source_map: bool,
    /// Pretty print output
    pub pretty: bool,
}

/// Emits Rust code from TypeScript source.
pub struct RustEmitter {
    /// Analysis results
    pub analysis: AnalysisResult,
    /// Imports needed by this module (path → names)
    pub imports: Vec<(String, Vec<String>)>,
    /// Output buffer
    output: String,
    /// Current indentation level
    indent: usize,
    /// Generation options
    options: CodegenOptions,
    /// Source line mappings for error translation
    source_line_map: Vec<(u32, u32)>,
}

impl RustEmitter {
    /// Create a new emitter.
    pub fn new(_source: &SourceFile, analysis: &AnalysisResult) -> Self {
        Self {
            analysis: analysis.clone(),
            imports: Vec::new(),
            output: String::new(),
            indent: 0,
            options: CodegenOptions::default(),
            source_line_map: Vec::new(),
        }
    }

    /// Emit the complete module using AST walking.
    pub fn emit(mut self, source: &SourceFile) -> crate::Result<GeneratedModule> {
        let file_stem = source
            .path
            .file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("module")
            .to_string();

        let module_name = AstWalker::escape_keyword(&file_stem);
        let source_text = source.source.clone();

        // Parse with SWC to get AST
        let ast = if source.is_tsx() {
            SwcAst::parse_tsx(&source_text, &file_stem)
        } else {
            SwcAst::parse_ts(&source_text, &file_stem)
        }
        .map_err(|e| crate::RuneError::Parse(crate::ParseError::Parse(format!("{e}"))))?;

        // Write header with imports
        self.write_header(&module_name);

        // Walk the AST and emit Rust code
        let mut walker = AstWalker::new();
        walker.walk_module(&ast.module);
        self.output.push_str(&walker.into_output());

        let output = std::mem::take(&mut self.output);
        let imports: Vec<_> = self
            .imports
            .into_iter()
            .map(|(path, names)| crate::codegen::Import {
                path,
                names: names
                    .into_iter()
                    .map(|n| crate::codegen::ImportedName {
                        original: n.clone(),
                        rust_name: n,
                    })
                    .collect(),
                is_native: false,
            })
            .collect();

        Ok(GeneratedModule {
            name: file_stem,
            source: output,
            imports,
            types: Vec::new(),
            functions: Vec::new(),
        })
    }

    /// Register a source line mapping for error translation.
    pub fn register_source_line(&mut self, generated_line: u32, source_line: u32) {
        self.source_line_map.push((generated_line, source_line));
    }

    /// Get source line mappings.
    pub fn source_line_map(&self) -> &[(u32, u32)] {
        &self.source_line_map
    }

    /// Write module header with imports.
    fn write_header(&mut self, module_name: &str) {
        self.push_line(&format!("// Module: {module_name}"));
        self.push_line("");
        // Protocol types - these are always needed for the app
        self.push_line("use protocol::{AppState, Filter, Task};");
        self.push_line("use ratatui::widgets::{Widget, Paragraph, ListItem};");
        self.push_line("use ratatui::style::{Style, Modifier};");
        self.push_line("use crossterm::event::KeyCode;");
        self.push_line("use serde_json;");
        self.push_line("use std::time;");
        self.push_line("");

        // Native imports
        self.push_line("use crate::native;");
        self.push_line("");
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
