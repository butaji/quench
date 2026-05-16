//! # Analysis Context
//!
//! Maintains state during analysis including source location tracking
//! and warning accumulation.

use crate::parser::SourceFile;
use super::TypeInfo;

/// Context for type and ownership analysis.
#[derive(Debug)]
pub struct AnalysisContext {
    /// Source file being analyzed
    source: SourceFile,
    /// Current line being processed
    current_line: u32,
    /// Current column being processed
    current_column: u32,
    /// Accumulated warnings
    warnings: Vec<super::AnalysisWarning>,
    /// Inferred types cache
    type_cache: std::collections::HashMap<String, TypeInfo>,
}

impl AnalysisContext {
    /// Create a new analysis context.
    pub fn new(source: &SourceFile) -> Self {
        Self {
            source: source.clone(),
            current_line: 1,
            current_column: 1,
            warnings: Vec::new(),
            type_cache: std::collections::HashMap::new(),
        }
    }

    /// Get the current source location as a string.
    pub fn current_location(&self) -> String {
        format!("{}:{}:{}", self.source.path.display(), self.current_line, self.current_column)
    }

    /// Update current location from a span.
    pub fn update_location(&mut self, span: &swc_common::Span) {
        let (line, col) = self.source.location_from_offset(span.lo.0);
        self.current_line = line;
        self.current_column = col;
    }

    /// Add a warning.
    pub fn add_warning(&mut self, location: String, message: String, code: &'static str) {
        self.warnings.push(super::AnalysisWarning {
            location,
            message,
            code,
        });
    }

    /// Take all warnings.
    pub fn take_warnings(&mut self) -> Vec<super::AnalysisWarning> {
        std::mem::take(&mut self.warnings)
    }

    /// Get inferred type for a name.
    pub fn infer_type(&self, _expr: &swc_ecma_ast::Expr) -> Option<TypeInfo> {
        // Would need to walk the expression tree to determine type
        None
    }

    /// Check if a string is a reserved Rust keyword.
    pub fn is_rust_keyword(&self, s: &str) -> bool {
        matches!(
            s,
            "as" | "async" | "await" | "break" | "const" | "continue" | "crate" | "dyn"
            | "else" | "enum" | "extern" | "false" | "fn" | "for" | "if" | "impl"
            | "in" | "let" | "loop" | "match" | "mod" | "move" | "mut" | "pub"
            | "ref" | "return" | "self" | "Self" | "static" | "struct" | "super"
            | "trait" | "true" | "type" | "unsafe" | "use" | "where" | "while"
        )
    }

    /// Mangle a name to avoid Rust keyword conflicts.
    pub fn mangle_name(&self, name: &str) -> String {
        if self.is_rust_keyword(name) {
            format!("{}_rune", name)
        } else {
            name.to_string()
        }
    }

    /// Get source file path.
    pub fn source_path(&self) -> &std::path::Path {
        &self.source.path
    }

    /// Get the raw source text.
    pub fn source_text(&self) -> &str {
        &self.source.source
    }
}
