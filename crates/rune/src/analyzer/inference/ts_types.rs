//! # Type Inference - TypeScript Types
//!
//! Type inference for TypeScript type annotations.

use super::{TypeInferrer, TypeInfo, StructInfo, EnumInfo};
use crate::analyzer::EnumVariant;

impl TypeInferrer {
    /// Parse a type annotation string.
    #[must_use]
    pub fn parse_ts_type(&self, type_str: &str) -> TypeInfo {
        let trimmed = type_str.trim();

        // Handle primitive types
        match trimmed {
            "number" => TypeInfo::Float,
            "string" => TypeInfo::String,
            "boolean" => TypeInfo::Boolean,
            "void" | "undefined" => TypeInfo::Unknown,
            "null" => TypeInfo::Option(Box::new(TypeInfo::Unknown)),
            "any" | "unknown" => TypeInfo::Unknown,
            _ => self.parse_complex_type(trimmed),
        }
    }

    /// Parse complex types like arrays, generics, unions.
    fn parse_complex_type(&self, type_str: &str) -> TypeInfo {
        let trimmed = type_str.trim();

        // Handle array types: T[]
        if let Some(inner) = trimmed.strip_suffix("[]") {
            return TypeInfo::Array(Box::new(self.parse_ts_type(inner)));
        }

        // Handle union types: T | null
        if trimmed.contains(" | ") {
            if trimmed.ends_with("| null") {
                let inner = trimmed.strip_suffix(" | null").unwrap_or(trimmed);
                return TypeInfo::Option(Box::new(self.parse_ts_type(inner)));
            }

            // Try to parse as tagged union
            if let Some(info) = self.parse_union_as_enum(trimmed) {
                return TypeInfo::Enum(info);
            }
        }

        // Handle Result pattern
        if trimmed.contains("ok:") && trimmed.contains("error:") {
            return TypeInfo::Result(
                Box::new(TypeInfo::Unknown),
                Box::new(TypeInfo::String),
            );
        }

        // Handle generic types: Name<T>
        if trimmed.ends_with('>') {
            if let Some((base, args)) = trimmed.split_once('<') {
                let args = args.trim_end_matches('>');
                return self.parse_generic_type(base, args);
            }
        }

        // Return as struct reference
        TypeInfo::Struct(StructInfo {
            name: trimmed.to_string(),
            fields: Vec::new(),
        })
    }

    /// Parse a union type as an enum if it has tag fields.
    fn parse_union_as_enum(&self, union: &str) -> Option<EnumInfo> {
        let variants: Vec<&str> = union.split(" | ").collect();

        let mut enum_variants = Vec::new();

        for variant in variants {
            let variant = variant.trim();
            if let Some(fields) = self.parse_tagged_variant(variant) {
                enum_variants.push(fields);
            } else {
                return None;
            }
        }

        Some(EnumInfo {
            name: "Union".to_string(),
            variants: enum_variants,
        })
    }

    /// Parse a tagged variant like `{ tag: "Name", ... }`.
    fn parse_tagged_variant(&self, variant: &str) -> Option<EnumVariant> {
        let variant = variant.trim();

        if !variant.starts_with('{') || !variant.ends_with('}') {
            return None;
        }

        let inner = &variant[1..variant.len() - 1];
        let mut tag = String::new();
        let mut fields = Vec::new();

        for field in inner.split(',') {
            let field = field.trim();
            if field.is_empty() {
                continue;
            }

            if let Some((key, value)) = field.split_once(':') {
                let key = key.trim();
                let value = value.trim();

                if key == "tag" && (value.starts_with('"') || value.starts_with('\'')) {
                    tag = value.trim_matches('"').trim_matches('\'').to_string();
                } else {
                    let type_info = self.parse_ts_type(value);
                    fields.push((key.to_string(), type_info));
                }
            }
        }

        if !tag.is_empty() {
            Some(EnumVariant { tag, fields })
        } else {
            None
        }
    }

    /// Parse a generic type.
    fn parse_generic_type(&self, base: &str, args: &str) -> TypeInfo {
        match base {
            "Array" | "Vec" => {
                TypeInfo::Array(Box::new(self.parse_ts_type(args)))
            }
            "Option" => {
                TypeInfo::Option(Box::new(self.parse_ts_type(args)))
            }
            "Result" => {
                if let Some((ok, err)) = args.split_once(", ") {
                    return TypeInfo::Result(
                        Box::new(self.parse_ts_type(ok)),
                        Box::new(self.parse_ts_type(err)),
                    );
                }
                TypeInfo::Unknown
            }
            _ => TypeInfo::Generic(base.to_string()),
        }
    }

    /// Infer struct fields from an object literal.
    #[must_use]
    pub fn infer_struct_fields(&self, obj: &str) -> Vec<(String, TypeInfo)> {
        let obj = obj.trim();
        if !obj.starts_with('{') || !obj.ends_with('}') {
            return Vec::new();
        }

        let inner = &obj[1..obj.len() - 1];
        let mut fields = Vec::new();

        for field in inner.split(',') {
            let field = field.trim();
            if field.is_empty() {
                continue;
            }

            if let Some((key, value)) = field.split_once(':') {
                let key = key.trim();
                let value = value.trim();
                let type_info = self.infer_literal(value);
                fields.push((key.to_string(), type_info));
            }
        }

        fields
    }
}
