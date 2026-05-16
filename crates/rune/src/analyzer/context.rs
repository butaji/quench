//! # Analysis Context
//!
//! Maintains state during analysis.

use crate::parser::SourceFile;
use crate::analyzer::TypeInfo;

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
    warnings: Vec<crate::analyzer::AnalysisWarning>,
}

impl AnalysisContext {
    /// Create a new analysis context.
    pub fn new(source: &SourceFile) -> Self {
        Self {
            source: source.clone(),
            current_line: 1,
            current_column: 1,
            warnings: Vec::new(),
        }
    }

    /// Get the current source location as a string.
    pub fn current_location(&self) -> String {
        format!("{}:{}:{}", self.source.path.display(), self.current_line, self.current_column)
    }

    /// Update current location from a span.
    #[allow(unused)]
    pub fn update_location(&mut self, _span: &()) {
        // Placeholder: In full implementation, would update from span
        self.current_line = 1;
        self.current_column = 1;
    }

    /// Add a warning.
    pub fn add_warning(&mut self, location: String, message: String, code: &'static str) {
        self.warnings.push(crate::analyzer::AnalysisWarning {
            location,
            message,
            code,
        });
    }

    /// Take all warnings.
    pub fn take_warnings(&mut self) -> Vec<crate::analyzer::AnalysisWarning> {
        std::mem::take(&mut self.warnings)
    }

    /// Get inferred type for a name.
    #[allow(unused)]
    pub fn infer_type(&self, _expr: &()) -> Option<TypeInfo> {
        // Placeholder: In full implementation, would infer type
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
