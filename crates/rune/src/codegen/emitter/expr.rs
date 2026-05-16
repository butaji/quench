//! # Expression Transpiler
//!
//! Transpiles TypeScript expressions to Rust expressions.

use super::{RustEmitter, to_snake_case};

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
pub struct ExprTranspiler<'a, 'em> {
    emitter: &'em RustEmitter<'a>,
}

impl<'a, 'em> ExprTranspiler<'a, 'em> {
    /// Create a new expression transpiler.
    #[must_use]
    pub fn new(emitter: &'em RustEmitter<'a>) -> Self {
        Self { emitter }
    }

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

    /// Transpile an expression to Rust.
    #[must_use]
    pub fn transpile(&self, expr: &str) -> String {
        let expr = expr.trim();

        match Self::classify(expr) {
            ExprKind::Literal => expr.to_string(),
            ExprKind::Identifier => to_snake_case(expr),
            ExprKind::BinaryOp => self.transpile_binary_op(expr),
            ExprKind::UnaryOp => self.transpile_unary_op(expr),
            ExprKind::Call => self.transpile_call(expr),
            ExprKind::PropertyAccess => Self::transpile_property_access(expr),
            ExprKind::ArrayLiteral => self.transpile_array_literal(expr),
            ExprKind::ObjectLiteral => self.transpile_object_literal(expr),
            ExprKind::Ternary => self.transpile_ternary(expr),
        }
    }

    /// Transpile a condition (handles &&, ||, ! specially).
    #[must_use]
    pub fn transpile_condition(&self, cond: &str) -> String {
        let cond = cond.trim();

        // Negation
        if let Some(inner) = cond.strip_prefix('!') {
            return format!("!{}", self.transpile_condition(inner.trim()));
        }

        // Parentheses
        if cond.starts_with('(') && cond.ends_with(')') {
            let inner = &cond[1..cond.len() - 1];
            return format!("({})", self.transpile_condition(inner));
        }

        self.transpile(cond)
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

    /// Transpile a binary operation.
    #[must_use]
    fn transpile_binary_op(&self, expr: &str) -> String {
        let op = Self::find_binary_op(expr).unwrap_or("+");
        let parts: Vec<&str> = expr.split(op).collect();

        if parts.len() == 2 {
            let left = parts[0].trim();
            let right = parts[1].trim();

            let rust_op = Self::map_operator(op);

            return format!(
                "({} {rust_op} {})",
                self.transpile(left),
                self.transpile(right)
            );
        }

        to_snake_case(expr)
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

    /// Transpile a unary operation.
    #[must_use]
    fn transpile_unary_op(&self, expr: &str) -> String {
        let expr = expr.trim();
        if let Some(inner) = expr.strip_prefix('!') {
            return format!("!{}", self.transpile(inner.trim()));
        }
        if let Some(inner) = expr.strip_prefix('-') {
            return format!("-{}", self.transpile(inner.trim()));
        }
        to_snake_case(expr)
    }

    /// Transpile a function call.
    #[must_use]
    fn transpile_call(&self, expr: &str) -> String {
        if let Some(paren_pos) = expr.find('(') {
            let func = &expr[..paren_pos];
            let args = expr[paren_pos + 1..]
                .trim_end_matches(')')
                .trim_end_matches(';');

            let rust_func = to_snake_case(func);
            let translated_args: Vec<String> = args.split(',')
                .map(|a| self.transpile(a.trim()))
                .collect();

            return format!("{rust_func}({})", translated_args.join(", "));
        }

        to_snake_case(expr)
    }

    /// Transpile property access.
    #[must_use]
    pub fn transpile_property_access(expr: &str) -> String {
        expr.split('.')
            .map(to_snake_case)
            .collect::<Vec<_>>()
            .join(".")
    }

    /// Transpile an array literal.
    #[must_use]
    fn transpile_array_literal(&self, expr: &str) -> String {
        let inner = expr.trim_start_matches('[').trim_end_matches(']');
        let elements: Vec<String> = inner
            .split(',')
            .map(|e| self.transpile(e.trim()))
            .collect();

        format!("vec![{}]", elements.join(", "))
    }

    /// Transpile an object literal.
    #[must_use]
    fn transpile_object_literal(&self, expr: &str) -> String {
        let inner = expr.trim_start_matches('{').trim_end_matches('}');
        let fields: Vec<String> = inner
            .split(',')
            .filter_map(|f| {
                let f = f.trim();
                if f.is_empty() {
                    return None;
                }
                let parts: Vec<&str> = f.split(':').collect();
                if parts.len() == 2 {
                    let key = parts[0].trim();
                    let value = self.transpile(parts[1].trim());
                    return Some(format!("{key}: {value}"));
                }
                Some(f.to_string())
            })
            .collect();

        format!("{{ {} }}", fields.join(", "))
    }

    /// Transpile a ternary expression.
    #[must_use]
    fn transpile_ternary(&self, expr: &str) -> String {
        let question_pos = expr.find(" ? ")
            .unwrap_or_else(|| expr.find("?(").unwrap_or(0));
        let colon_pos = expr.rfind(" : ")
            .unwrap_or_else(|| expr.rfind(":(").unwrap_or(0));

        if colon_pos > question_pos {
            let condition = self.transpile_condition(&expr[..question_pos]);
            let then_expr = expr[question_pos + 3..colon_pos].trim();
            let else_expr = expr[colon_pos + 3..]
                .trim_end_matches(';')
                .trim_end_matches(')');

            return format!(
                "if {} {{ {} }} else {{ {} }}",
                condition,
                self.transpile(then_expr),
                self.transpile(else_expr)
            );
        }

        to_snake_case(expr)
    }
}
