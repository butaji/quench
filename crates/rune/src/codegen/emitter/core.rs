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

    /// Add protocol import.
    #[must_use]
    pub fn with_protocol_import(mut self) -> Self {
        self.custom_imports
            .push("use crate::protocol::{{AppState, Task, Filter}};".into());
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
        Self {
            analysis: analysis.clone(),
            imports: Vec::new(),
            output: String::new(),
            indent: 0,
            options: CodegenOptions::default(),
            source_line_map: Vec::new(),
            emit_options: EmitOptions::new(),
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
        let file_stem = source
            .path
            .file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("module")
            .to_string();

        let module_name = escape_keyword(&file_stem);
        let source_text = source.source.clone();

        // Parse with SWC to get AST
        let ast = if source.is_tsx() {
            SwcAst::parse_tsx(&source_text, &file_stem)
        } else {
            SwcAst::parse_ts(&source_text, &file_stem)
        }
        .map_err(|e| crate::RuneError::Parse(crate::ParseError::Parse(format!("{e}"))))?;

        // Walk the AST and emit Rust code
        let mut walker = AstWalker::new();
        walker.walk_module(&ast.module);

        // Get native imports from walker
        let native_imports = { walker.native_imports().clone() };

        // Write header with imports
        self.write_header(&module_name, &native_imports);

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
    #[must_use]
    pub fn source_line_map(&self) -> &[(u32, u32)] {
        &self.source_line_map
    }

    /// Write module header with imports.
    fn write_header(&mut self, module_name: &str, native_imports: &HashSet<String>) {
        self.push_line(&format!("// Module: {module_name}"));
        self.push_line("");

        // Add custom imports (framework-specific imports come from config)
        let custom_imports = self.emit_options.custom_imports.clone();
        for import in custom_imports {
            self.push_line(&import);
        }

        if !self.emit_options.custom_imports.is_empty() {
            self.push_line("");
        }

        // Native module import for hand-written Rust functions
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
