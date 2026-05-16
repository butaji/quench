//! # TypeScript Type Inference
//!
//! Infers Rust types from TypeScript type annotations.

use crate::analyzer::TypeInfo;

/// Infers type from a TypeScript type annotation string.
#[allow(unused)]
pub fn infer_ts_type(type_str: &str) -> TypeInfo {
    let s = type_str.trim();

    // Handle void/undefined
    if s == "void" || s == "undefined" {
        return TypeInfo::Unknown;
    }

    // Primitive types
    match s {
        "number" => TypeInfo::Float,
        "string" => TypeInfo::String,
        "boolean" => TypeInfo::Boolean,
        "null" => TypeInfo::Unknown,
        _ => TypeInfo::Unknown,
    }
}
