//! # Type Emitter
//!
//! Emits Rust type declarations from TypeScript types.

use crate::analyzer::TypeInfo;
use super::emitter::utils::{to_snake_case as util_snake, to_pascal_case as util_pascal};

/// Emits Rust type code.
#[allow(dead_code)]
pub struct TypeEmitter;

impl TypeEmitter {
    #[allow(dead_code)]
    /// Create a new type emitter.
    pub fn new() -> Self {
        Self
    }

    /// Emit a type declaration.
    pub fn emit_type(&self, type_info: &TypeInfo) -> String {
        match type_info {
            TypeInfo::Integer(_) => "i32".to_string(),
            TypeInfo::Float => "f64".to_string(),
            TypeInfo::String => "String".to_string(),
            TypeInfo::StringLiteral(s) => format!("&str // literal: {}", s),
            TypeInfo::Boolean => "bool".to_string(),
            TypeInfo::Array(elem) => format!("Vec<{}>", self.emit_type(elem)),
            TypeInfo::Struct(s) => self.emit_struct(s),
            TypeInfo::Enum(e) => self.emit_enum(e),
            TypeInfo::Option(inner) => format!("Option<{}>", self.emit_type(inner)),
            TypeInfo::Result(ok, err) => format!("Result<{}, {}>", self.emit_type(ok), self.emit_type(err)),
            TypeInfo::Function(f) => self.emit_function(f),
            TypeInfo::Generic(name) => name.clone(),
            TypeInfo::Unknown => "()".to_string(),
        }
    }

    /// Emit a struct.
    #[allow(dead_code)]
    fn emit_struct(&self, s: &crate::analyzer::StructInfo) -> String {
        let fields = s.fields.iter()
            .map(|(name, ty)| format!("    pub {}: {},", self.to_snake_case(name), self.emit_type(ty)))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "struct {} {{\n{}\n}}",
            self.to_pascal_case(&s.name),
            fields
        )
    }

    /// Emit an enum.
    #[allow(dead_code)]
    fn emit_enum(&self, e: &crate::analyzer::EnumInfo) -> String {
        let variants = e.variants.iter()
            .map(|v| {
                if v.fields.is_empty() {
                    format!("    {},", self.to_pascal_case(&v.tag))
                } else {
                    let fields = v.fields.iter()
                        .map(|(n, t)| format!("{}: {}", self.to_snake_case(n), self.emit_type(t)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("    {} {{ {} }},", self.to_pascal_case(&v.tag), fields)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "enum {} {{\n{}\n}}",
            self.to_pascal_case(&e.name),
            variants
        )
    }

    /// Emit a function type.
    #[allow(dead_code)]
    fn emit_function(&self, f: &crate::analyzer::FunctionInfo) -> String {
        let params = f.params.iter()
            .map(|(n, t)| format!("{}: {}", self.to_snake_case(n), self.emit_type(t)))
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            "fn({}) -> {}",
            params,
            self.emit_type(&f.return_type)
        )
    }

    /// Convert to snake_case using shared utility.
    #[allow(dead_code)]
    fn to_snake_case(&self, s: &str) -> String {
        util_snake(s)
    }

    /// Convert to PascalCase using shared utility.
    #[allow(dead_code)]
    fn to_pascal_case(&self, s: &str) -> String {
        util_pascal(s)
    }
}

impl Default for TypeEmitter {
    fn default() -> Self {
        Self::new()
    }
}
