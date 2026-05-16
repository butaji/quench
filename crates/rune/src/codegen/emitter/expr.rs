//! # Expression Transpiler
//!
//! Transpiles TypeScript expressions to Rust expressions.
//!
//! This module is deprecated - use AstWalker instead.

/// Expression kind classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExprKind {
    /// Literal value
    Literal,
    /// Identifier/variable
    Identifier,
    /// Binary operation
    BinaryOp,
    /// Unary operation
    UnaryOp,
    /// Function call
    Call,
    /// Property access
    PropertyAccess,
    /// Array literal
    ArrayLiteral,
    /// Object literal
    ObjectLiteral,
    /// Ternary expression
    Ternary,
}

/// Transpiles expressions.
pub struct ExprTranspiler;

impl ExprTranspiler {
    /// Classify an expression.
    #[must_use]
    pub fn classify(expr: &str) -> ExprKind {
        let expr = expr.trim();

        if expr.starts_with('"') || expr.starts_with('\'') {
            return ExprKind::Literal;
        }
        if expr.parse::<f64>().is_ok() {
            return ExprKind::Literal;
        }
        if expr == "true" || expr == "false" {
            return ExprKind::Literal;
        }
        if expr.contains(" ? ") || (expr.starts_with('(') && expr.contains('?')) {
            return ExprKind::Ternary;
        }
        if Self::find_binary_op(expr).is_some() {
            return ExprKind::BinaryOp;
        }
        if expr.starts_with('!') || expr.starts_with('-') {
            return ExprKind::UnaryOp;
        }
        if expr.contains('(') {
            return ExprKind::Call;
        }
        if expr.starts_with('[') {
            return ExprKind::ArrayLiteral;
        }
        if expr.starts_with('{') {
            return ExprKind::ObjectLiteral;
        }
        if expr.contains('.') && !expr.contains('(') {
            return ExprKind::PropertyAccess;
        }

        ExprKind::Identifier
    }

    /// Find binary operator in expression.
    #[must_use]
    pub fn find_binary_op(expr: &str) -> Option<&'static str> {
        const OPS: &[&str] = &[
            "+=", "-=", "*=", "/=", "===", "!==", "==", "!=",
            "<=", ">=", "&&", "||", "+", "-", "*", "/", "%",
            ">", "<", "|", "&", "^",
        ];

        OPS.iter().find(|&&op| expr.contains(op)).copied()
    }

    /// Map TypeScript operator to Rust.
    #[must_use]
    pub fn map_operator(op: &str) -> String {
        match op {
            "===" | "==" => "==".to_string(),
            "!==" | "!=" => "!=".to_string(),
            "&&" => "&&".to_string(),
            "||" => "||".to_string(),
            "+" | "-" | "*" | "/" | "%" | "|" | "&" | "^" => op.to_string(),
            "<" | ">" | "<=" | ">=" => op.to_string(),
            _ => op.to_string(),
        }
    }
}
