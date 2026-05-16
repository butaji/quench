//! # Core Emitter
//!
//! Main RustEmitter implementation.

use crate::{parser::SourceFile, analyzer::AnalysisResult};
use crate::codegen::{GeneratedModule, CodegenOptions};

/// Options for code emission.
#[derive(Debug, Clone, Default)]
pub struct EmitOptions {
    /// Generate debug info
    pub source_map: bool,
    /// Pretty print output
    pub pretty: bool,
}

/// Emits Rust code from TypeScript source.
#[derive(Debug)]
pub struct RustEmitter<'a> {
    /// Source file being transpiled
    pub(crate) source: &'a SourceFile,
    /// Analysis results
    pub(crate) analysis: &'a AnalysisResult,
    /// Imports needed by this module
    pub(crate) imports: Vec<crate::codegen::Import>,
    /// Output buffer
    pub(crate) output: String,
    /// Current indentation level
    pub(crate) indent: usize,
    /// Generation options
    #[allow(unused)]
    options: CodegenOptions,
}

impl<'a> RustEmitter<'a> {
    /// Create a new emitter.
    #[must_use]
    pub fn new(source: &'a SourceFile, analysis: &'a AnalysisResult) -> Self {
        Self {
            source,
            analysis,
            imports: Vec::new(),
            output: String::new(),
            indent: 0,
            options: CodegenOptions::default(),
        }
    }

    /// Emit the complete module.
    ///
    /// # Errors
    /// Returns an error if code generation fails.
    pub fn emit(mut self) -> crate::Result<GeneratedModule> {
        use super::module;
        self.write_header();
        module::write_types(&mut self);
        module::write_functions(&mut self);
        self.write_footer();

        let name = self.source.path.file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("module")
            .to_string();

        Ok(GeneratedModule {
            name,
            source: self.output,
            imports: self.imports,
            types: Vec::new(),
            functions: Vec::new(),
        })
    }

    /// Write module header with imports.
    fn write_header(&mut self) {
        use super::module;
        module::write_header(self);
    }

    /// Write type definitions.
    fn write_types(&mut self) {
        use super::module;
        module::write_types(self);
    }

    /// Write function definitions.
    fn write_functions(&mut self) {
        use super::module;
        module::write_functions(self);
    }

    /// Write module footer.
    fn write_footer(&mut self) {
        self.push_line("// End of generated code");
    }

    /// Emit a single function definition.
    fn emit_function(&mut self, name: &str, func: &crate::analyzer::FunctionInfo) {
        use super::module;
        module::emit_function(self, name, func);
    }

    /// Generate function body from source.
    fn generate_function_body(&mut self, name: &str) {
        use super::stmt::StmtTranspiler;
        StmtTranspiler::generate_function_body(self, name);
    }

    /// Translate a TypeScript line to Rust.
    fn translate_line(&mut self, line: &str) {
        use super::stmt::StmtTranspiler;
        StmtTranspiler::translate_line(self, line);
    }

    /// Translate an expression.
    fn translate_expr(&self, expr: &str) -> String {
        use super::expr::ExprTranspiler;
        ExprTranspiler::new(self).transpile(expr)
    }

    /// Translate a condition.
    fn translate_condition(&self, cond: &str) -> String {
        use super::expr::ExprTranspiler;
        ExprTranspiler::new(self).transpile_condition(cond)
    }

    pub(crate) fn push_line(&mut self, s: &str) {
        self.output.push_str(s);
        self.output.push('\n');
    }

    pub(crate) fn push_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
    }
}
