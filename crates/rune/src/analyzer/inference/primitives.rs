//! # Primitive Type Inference
//!
//! Infers types from literals and primitives.

use swc_ecma_ast::*;
use crate::analyzer::{TypeInfo, StructInfo, EnumInfo, EnumVariant, FunctionInfo};

/// Infers types from literals.
pub fn infer_lit(lit: &Lit) -> TypeInfo {
    match lit {
        Lit::Num(n) => {
            if n.value.fract() == 0.0 && n.value.abs() <= i32::MAX as f64 {
                TypeInfo::Integer(n.value as i64)
            } else {
                TypeInfo::Float
            }
        }
        Lit::Str(s) => TypeInfo::StringLiteral(s.value.to_string()),
        Lit::Bool(_) => TypeInfo::Boolean,
        Lit::Null(_) => TypeInfo::Unknown,
        Lit::BigInt(_) => TypeInfo::Integer(0),
        _ => TypeInfo::Unknown,
    }
}

/// Infers type from a binary expression.
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
pub fn infer_bin_op_result(op: BinaryOp) -> TypeInfo {
    match op {
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div
        | BinaryOp::Mod | BinaryOp::Exp | BinaryOp::NullishCoalescing => TypeInfo::Float,
        BinaryOp::EqEq | BinaryOp::NotEq | BinaryOp::EqEqEq | BinaryOp::NotEqEq
        | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge
        | BinaryOp::LogicalAnd | BinaryOp::LogicalOr
        | BinaryOp::BinAnd | BinaryOp::BinOr | BinaryOp::BinXor
        | BinaryOp::LShift | BinaryOp::RShift | BinaryOp::ZeroFillRShift => TypeInfo::Boolean,
    }
}
