//! # Analysis Context
//!
//! Maintains state during analysis.

use super::{AnalysisWarning, TypeInfo};
use crate::parser::SourceFile;
use crate::utils::escape_rust_keyword;

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
    warnings: Vec<AnalysisWarning>,
    /// Scope stack for variable resolution
    scopes: Vec<std::collections::HashMap<String, TypeInfo>>,
}

impl AnalysisContext {
    /// Create a new analysis context.
    #[must_use]
    pub fn new(source: &SourceFile) -> Self {
        Self {
            source: source.clone(),
            current_line: 1,
            current_column: 1,
            warnings: Vec::new(),
            scopes: vec![std::collections::HashMap::new()],
        }
    }

    /// Get the current source location as a string.
    #[must_use]
    pub fn current_location(&self) -> String {
        format!(
            "{}:{}:{}",
            self.source.path.display(),
            self.current_line,
            self.current_column
        )
    }

    /// Update current location from line and column.
    pub fn set_location(&mut self, line: u32, column: u32) {
        self.current_line = line;
        self.current_column = column;
    }

    /// Add a warning.
    pub fn add_warning(&mut self, location: String, message: String, code: &'static str) {
        self.warnings.push(AnalysisWarning {
            location,
            message,
            code,
        });
    }

    /// Add a warning at current location.
    pub fn warn(&mut self, message: String, code: &'static str) {
        self.add_warning(self.current_location(), message, code);
    }

    /// Take all warnings.
    #[must_use]
    pub fn take_warnings(&mut self) -> Vec<AnalysisWarning> {
        std::mem::take(&mut self.warnings)
    }

    /// Push a new scope.
    pub fn push_scope(&mut self) {
        self.scopes.push(std::collections::HashMap::new());
    }

    /// Pop the current scope.
    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// Declare a variable in the current scope.
    pub fn declare(&mut self, name: String, info: TypeInfo) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, info);
        }
    }

    /// Resolve a variable name in scopes.
    #[must_use]
    pub fn resolve(&self, name: &str) -> Option<&TypeInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info);
            }
        }
        None
    }

    /// Check if a string is a reserved Rust keyword.
    #[must_use]
    pub fn is_rust_keyword(&self, s: &str) -> bool {
        escape_rust_keyword(s) != s
    }

    /// Mangle a name to avoid Rust keyword conflicts.
    #[must_use]
    pub fn mangle_name(&self, name: &str) -> String {
        escape_rust_keyword(name)
    }

    /// Get source file path.
    #[must_use]
    pub fn source_path(&self) -> &std::path::Path {
        &self.source.path
    }

    /// Get the raw source text.
    #[must_use]
    pub fn source_text(&self) -> &str {
        &self.source.source
    }

    /// Check if this is a TSX file.
    #[must_use]
    pub fn is_tsx(&self) -> bool {
        self.source.is_tsx()
    }
}
