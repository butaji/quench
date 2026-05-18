//! # Code Generation Types
//!
//! Shared type definitions for code generation.

use crate::utils::to_snake_case;
use std::fmt;

/// Rust type representation for code generation.
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

impl RustType {
    /// Convert to a string representation suitable for use in generic parameters.
    #[must_use]
    pub fn to_rust_type_string(&self) -> String {
        match self {
            RustType::I32 => "i32".to_string(),
            RustType::F64 => "f64".to_string(),
            RustType::Bool => "bool".to_string(),
            RustType::String => "String".to_string(),
            RustType::Str => "&str".to_string(),
            RustType::Vec(t) => format!("Vec<{}>", t.to_rust_type_string()),
            RustType::Option(t) => format!("Option<{}>", t.to_rust_type_string()),
            RustType::Result(t) => format!("Result<{}, String>", t.to_rust_type_string()),
            RustType::HashMap(k, v) => {
                format!(
                    "std::collections::HashMap<{}, {}>",
                    k.to_rust_type_string(),
                    v.to_rust_type_string()
                )
            }
            RustType::Unit | RustType::Unknown => "()".to_string(),
            RustType::Custom(name) => name.clone(),
            RustType::MutBorrow(t) => format!("&mut {}", t.to_rust_type_string()),
        }
    }
}

/// Raw field for deferred type resolution.
#[allow(dead_code)]
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
#[allow(dead_code)]
#[must_use]
pub fn is_enum_type(name: &str) -> bool {
    crate::utils::is_enum_type(name)
}

/// Convert a type/variant name to appropriate Rust form.
#[allow(dead_code)]
#[must_use]
pub fn to_rust_name(name: &str) -> String {
    if is_enum_type(name) {
        name.to_string()
    } else {
        to_snake_case(name)
    }
}
