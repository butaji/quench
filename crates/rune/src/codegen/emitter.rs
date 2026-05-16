//! # Rust Emitter
//!
//! Core transpilation from TypeScript AST to Rust source.

use crate::{parser::SourceFile, analyzer::AnalysisResult};
use super::{GeneratedModule, Import, ImportedName};
use super::emitters::{ExprEmitter, StmtEmitter, TypeEmitter};

/// Options for code emission.
#[derive(Debug, Clone, Default)]
pub struct EmitOptions {
    pub source_map: bool,
    pub pretty: bool,
}

/// Emits Rust code from TypeScript AST.
pub struct RustEmitter<'a> {
    source: &'a SourceFile,
    analysis: &'a AnalysisResult,
    imports: Vec<Import>,
    output: String,
    indent: usize,
    type_emitter: TypeEmitter,
}

impl<'a> RustEmitter<'a> {
    /// Create a new emitter.
    pub fn new(source: &'a SourceFile, analysis: &'a AnalysisResult) -> Self {
        Self {
            source,
            analysis,
            imports: Vec::new(),
            output: String::new(),
            indent: 0,
            type_emitter: TypeEmitter::new(),
        }
    }

    /// Emit the complete module.
    pub fn emit(mut self) -> crate::Result<GeneratedModule> {
        self.write_header()?;
        self.write_footer()?;

        let name = self.source.path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("module")
            .to_string();

        Ok(GeneratedModule {
            name,
            source: self.output,
            imports: self.imports,
        })
    }

    /// Write module header.
    fn write_header(&mut self) -> crate::Result<()> {
        self.push_line("use std::collections::HashMap;");
        self.push_line("use std::fmt::{self, Write};");
        self.push_line("");
        Ok(())
    }

    /// Write module footer.
    fn write_footer(&mut self) -> crate::Result<()> {
        // Add placeholder for transpiled code
        self.push_line("// TODO: Add transpiled TypeScript code here");
        Ok(())
    }

    fn push(&mut self, s: &str) {
        self.output.push_str(s);
    }

    fn push_line(&mut self, s: &str) {
        self.output.push_str(s);
        self.output.push('\n');
    }
}
