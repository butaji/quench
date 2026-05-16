//! # Parse Diagnostics
//!
//! Collects and formats parse errors.

use crate::ParseError;

/// Accumulated parse diagnostics.
#[derive(Debug, Default)]
pub struct ParseDiagnostics {
    errors: Vec<ParseError>,
}

impl ParseDiagnostics {
    /// Create a new diagnostics collector.
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    /// Add an error.
    pub fn add_error(&mut self, err: ParseError) {
        self.errors.push(err);
    }

    /// Check if there are any errors.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Get all errors.
    pub fn errors(&self) -> &[ParseError] {
        &self.errors
    }

    /// Take all errors, leaving an empty collector.
    pub fn take_errors(&mut self) -> Vec<ParseError> {
        std::mem::take(&mut self.errors)
    }

    /// Format errors as a string.
    pub fn format(&self) -> String {
        self.errors
            .iter()
            .map(|e| format!("  - {e}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl std::fmt::Display for ParseDiagnostics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Parse errors:\n{}", self.format())
    }
}
