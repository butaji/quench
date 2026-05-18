//! # Type Inference - Primitives
//!
//! Type inference for primitive types.

use super::{TypeInferrer, TypeInfo};

impl TypeInferrer {
    /// Infer type from a literal value.
    #[must_use]
    pub fn infer_literal(&self, literal: &str) -> TypeInfo {
        // Try integer
        if let Ok(n) = literal.parse::<i64>() {
            return TypeInfo::Integer(n);
        }

        // Try float
        if literal.parse::<f64>().is_ok() {
            return TypeInfo::Float;
        }

        // String literal
        if literal.starts_with('"') || literal.starts_with('\'') {
            let content = &literal[1..literal.len() - 1];
            return TypeInfo::StringLiteral(content.to_string());
        }

        // Boolean
        if literal == "true" || literal == "false" {
            return TypeInfo::Boolean;
        }

        TypeInfo::Unknown
    }

    /// Infer type from binary operator.
    #[allow(dead_code)]
    #[must_use]
    pub fn infer_bin_expr_type(left: &TypeInfo, right: &TypeInfo) -> TypeInfo {
        use TypeInfo::*;

        match (left, right) {
            // Number + Number = Number
            (Integer(_), Integer(_)) => Integer(0),
            (Float, Integer(_)) | (Integer(_), Float) => Float,
            (Float, Float) => Float,
            // String + anything = String
            (String | StringLiteral(_), _) | (_, String | StringLiteral(_)) => String,
            // Boolean operations
            (Boolean, Boolean) => Boolean,
            _ => Unknown,
        }
    }
}
