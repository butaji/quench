//! # Primitive Type Inference
//!
//! Infers types from literals and primitives.

use crate::analyzer::TypeInfo;

/// Infers types from literals.
#[allow(unused)]
pub fn infer_lit(lit: &()) -> TypeInfo {
    // Placeholder: In full implementation, would inspect literal
    TypeInfo::Unknown
}

/// Infers type from a binary expression.
#[allow(unused)]
pub fn infer_bin_expr_type(left: &TypeInfo, right: &TypeInfo) -> TypeInfo {
    // If either is Float, result is Float
    if matches!(left, TypeInfo::Float) || matches!(right, TypeInfo::Float) {
        return TypeInfo::Float;
    }
    // If both are integers, result is integer
    if matches!(left, TypeInfo::Integer(_)) && matches!(right, TypeInfo::Integer(_)) {
        return TypeInfo::Integer(0);
    }
    // String concatenation
    if matches!(left, TypeInfo::String | TypeInfo::StringLiteral(_))
        || matches!(right, TypeInfo::String | TypeInfo::StringLiteral(_)) {
        return TypeInfo::String;
    }
    TypeInfo::Float
}

/// Infers the result type of a binary operator.
#[allow(unused)]
pub fn infer_bin_op_result(op: &str) -> TypeInfo {
    match op {
        "+" | "-" | "*" | "/" | "%" => TypeInfo::Float,
        "==" | "!=" | "<" | "<=" | ">" | ">=" | "&&" | "||" => TypeInfo::Boolean,
        _ => TypeInfo::Unknown,
    }
}
