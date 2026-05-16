//! # Expression Emitter
//!
//! Emits Rust code from TypeScript expressions.

use crate::analyzer::AnalysisResult;

/// Emits Rust code for expressions.
pub struct ExprEmitter<'a> {
    /// Output buffer
    output: String,
    /// Current indentation
    #[allow(unused)]
    indent: usize,
    /// Analysis result
    _analysis: &'a AnalysisResult,
}

impl<'a> ExprEmitter<'a> {
    /// Create a new expression emitter.
    #[must_use]
    pub const fn new(analysis: &'a AnalysisResult) -> Self {
        Self {
            output: String::new(),
            indent: 0,
            _analysis: analysis,
        }
    }

    /// Emit an expression.
    #[allow(unused)]
    #[must_use]
    pub fn emit_expr(&mut self, expr: &str) -> String {
        self.emit_string(expr);
        self.output.clone()
    }

    /// Emit an expression string to output.
    fn emit_string(&mut self, expr: &str) {
        self.output.push_str(expr);
    }

    /// Get the output.
    #[must_use]
    pub fn into_output(self) -> String {
        self.output
    }
}
