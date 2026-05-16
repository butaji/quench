//! # Rust Type Representations
//!
//! Type information for code generation.

use crate::codegen::emitter::utils::to_snake_case;
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

/// Check if a name looks like an enum type (PascalCase).
/// These should be preserved as-is for enum variants.
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
