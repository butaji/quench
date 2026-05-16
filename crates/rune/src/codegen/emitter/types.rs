//! # Rust Type Representations
//!
//! Type information for code generation.

use std::fmt;

/// Type information for code generation.
#[derive(Debug, Clone)]
pub enum RustType {
    I32,
    F64,
    Bool,
    String,
    Str,
    Vec(Box<RustType>),
    Option(Box<RustType>),
    Result(Box<RustType>),
    HashMap(Box<RustType>, Box<RustType>),
    Unit,
    Unknown,
    Custom(String),
    /// Mutable borrow of a type
    MutBorrow(Box<RustType>),
}

impl fmt::Display for RustType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RustType::I32 => write!(f, "i32"),
            RustType::F64 => write!(f, "f64"),
            RustType::Bool => write!(f, "bool"),
            RustType::String => write!(f, "String"),
            RustType::Str => write!(f, "&str"),
            RustType::Vec(t) => write!(f, "Vec<{t}>"),
            RustType::Option(t) => write!(f, "Option<{t}>"),
            RustType::Result(t) => write!(f, "Result<{t}, String>"),
            RustType::HashMap(k, v) => write!(f, "std::collections::HashMap<{k}, {v}>"),
            RustType::Unit | RustType::Unknown => write!(f, "()"),
            RustType::Custom(name) => write!(f, "{name}"),
            RustType::MutBorrow(t) => write!(f, "&mut {t}"),
        }
    }
}

/// Raw field for deferred type resolution.
pub type RawField = (String, swc_ecma_ast::TsType);

/// Struct field info.
pub type StructFields = Vec<(String, RustType)>;

/// Enum variant.
#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub fields: StructFields,
}

/// Enum definition.
#[derive(Debug, Clone)]
pub struct EnumDefinition {
    pub name: String,
    pub variants: Vec<EnumVariant>,
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

/// Check if a name looks like an enum type (PascalCase).
/// These should be preserved as-is for enum variants.
#[must_use]
pub fn is_enum_type(name: &str) -> bool {
    // PascalCase: first char is uppercase, rest is alphanumeric
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_uppercase() => chars.all(|c| c.is_alphanumeric()),
        _ => false,
    }
}

/// Convert a type/variant name to appropriate Rust form.
/// - Enum types (PascalCase) are preserved: Filter -> Filter
/// - Enum variants (PascalCase) become PascalCase: Active -> Active
/// - Regular names become snake_case: filter_tasks -> filter_tasks
#[must_use]
pub fn to_rust_name(name: &str) -> String {
    // Preserve case for PascalCase names (likely enum types/variants)
    if is_enum_type(name) {
        name.to_string()
    } else {
        to_snake_case(name)
    }
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
