//! # Shared Utilities
//!
//! Common utilities for TypeScript to Rust transpilation.
//! This module is shared by both analyzer and codegen layers.

// Extended Rust keyword set covering all reserved words
const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while", // Extended set for compatibility
    "abstract", "become", "box", "do", "final", "macro", "override", "priv", "try", "typeof",
    "unsized", "virtual", "yield",
];

/// Escape a Rust keyword for use as an identifier.
/// Uses the extended keyword set covering all Rust reserved words.
#[must_use]
pub fn escape_rust_keyword(name: &str) -> String {
    if RUST_KEYWORDS.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

/// Escape a Rust keyword for module names.
/// Alias for escape_rust_keyword since both use the same rules.
#[must_use]
pub fn escape_rust_keyword_for_module(name: &str) -> String {
    escape_rust_keyword(name)
}

/// Convert name to snake_case.
#[must_use]
pub fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(c.to_ascii_lowercase());
    }
    result
}

/// Convert name to PascalCase.
#[must_use]
pub fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '_' || c == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Check if a name looks like an enum type (PascalCase).
#[must_use]
pub fn is_enum_type(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_uppercase() => chars.all(|c| c.is_alphanumeric()),
        _ => false,
    }
}

/// Convert a type/variant name to appropriate Rust form.
#[must_use]
pub fn to_rust_name(name: &str) -> String {
    if is_enum_type(name) {
        name.to_string()
    } else {
        to_snake_case(name)
    }
}

/// Escape a Rust keyword for use as an identifier in AST walker.
#[must_use]
pub fn escape_keyword(name: &str) -> String {
    escape_rust_keyword(name)
}

#[cfg(test)]
mod tests;
