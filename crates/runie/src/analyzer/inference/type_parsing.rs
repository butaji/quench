//! # Type Parsing Module
//!
//! Parses primitive types and literal types.

use crate::analyzer::TypeInfo;

/// Parse primitive TypeScript types.
pub fn parse_primitive_type(s: &str) -> Option<Box<TypeInfo>> {
    match s {
        "void" | "undefined" | "null" => Some(Box::new(TypeInfo::Unknown)),
        "number" => Some(Box::new(TypeInfo::Float)),
        "string" => Some(Box::new(TypeInfo::String)),
        "boolean" => Some(Box::new(TypeInfo::Boolean)),
        _ => None,
    }
}

/// Parse string literal types (e.g., `"foo"`).
pub fn parse_string_literal(s: &str) -> Option<Box<TypeInfo>> {
    let is_quoted =
        (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\''));
    if is_quoted {
        Some(Box::new(TypeInfo::StringLiteral(
            s[1..s.len() - 1].to_string(),
        )))
    } else {
        None
    }
}

/// Parse integer literal types (e.g., `42`).
pub fn parse_integer_literal(s: &str) -> Option<Box<TypeInfo>> {
    s.parse::<i64>()
        .ok()
        .map(|n| Box::new(TypeInfo::Integer(n)))
}
