//! # Statement Transpiler
//!
//! Transpiles TypeScript statements to Rust statements.
//!
//! This module is deprecated - use AstWalker instead.

/// Extract substring until a closing character.
#[must_use]
fn rest_until(s: &str, start: usize, end: char) -> &str {
    if let Some(end_pos) = s[start..].find(end) {
        &s[start..start + end_pos]
    } else {
        &s[start..]
    }
}

/// Transpiles statements.
pub struct StmtTranspiler;

impl StmtTranspiler {
    /// Parse a type hint string.
    #[must_use]
    pub fn parse_type_hint(hint: &str) -> String {
        match hint {
            "number" => "f64".to_string(),
            "string" => "String".to_string(),
            "boolean" => "bool".to_string(),
            "i32" | "integer" => "i32".to_string(),
            "usize" => "usize".to_string(),
            "void" | "undefined" => "()".to_string(),
            _ => hint.to_string(),
        }
    }
}
