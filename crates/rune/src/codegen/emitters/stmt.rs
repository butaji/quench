//! # Statement Emitter
//!
//! Emits Rust code from TypeScript statements.

use crate::analyzer::AnalysisResult;
use super::expr::ExprEmitter;

/// Emits Rust code for statements.
pub struct StmtEmitter<'a> {
    /// Output buffer
    output: String,
    /// Current indentation
    indent: usize,
    /// Expression emitter reference
    #[allow(unused)]
    expr_emitter: ExprEmitter<'a>,
}

impl<'a> StmtEmitter<'a> {
    /// Create a new statement emitter.
    #[must_use]
    pub const fn new(analysis: &'a AnalysisResult) -> Self {
        Self {
            output: String::new(),
            indent: 0,
            expr_emitter: ExprEmitter::new(analysis),
        }
    }

    /// Emit a statement.
    #[allow(unused)]
    pub fn emit_stmt(&mut self, stmt: &str) {
        self.push_indent();
        self.push(stmt);
        self.push_line(";");
    }

    /// Get the output.
    #[must_use]
    pub fn into_output(self) -> String {
        self.output
    }

    fn push(&mut self, s: &str) {
        self.output.push_str(s);
    }

    fn push_line(&mut self, s: &str) {
        self.output.push_str(s);
        self.output.push('\n');
    }

    fn push_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
    }
}
