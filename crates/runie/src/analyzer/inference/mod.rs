//! # Type Inference Module
//!
//! Infers Rust types from TypeScript source.

mod complex_types;
mod parser_helpers;
mod primitives;
mod struct_parsing;
mod ts_types;
mod type_parsing;

use crate::analyzer::{TypeInfo, TypeMap};
use crate::parser::SourceFile;
pub use parser_helpers::{parse_enum, parse_function, parse_interfaces, parse_type_alias};

/// Main type inference engine.
#[derive(Debug)]
pub struct TypeInferrer {
    /// Inferred types for this file
    types: TypeMap,
    /// Module name
    module_name: String,
}

impl TypeInferrer {
    /// Create a new type inferrer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            types: TypeMap::default(),
            module_name: String::new(),
        }
    }

    /// Infer all types from source text.
    pub fn infer_from_source(&mut self, source: &SourceFile) -> crate::Result<TypeMap> {
        self.module_name = source.module_name().to_string();
        let mut types = TypeMap::default();

        // Parse types from source text using regex-like patterns
        self.infer_types_from_text(&source.source, &mut types);

        self.types = types.clone();
        Ok(types)
    }

    /// Infer types from source text using simple pattern matching.
    fn infer_types_from_text(&self, source: &str, types: &mut TypeMap) {
        // Find function declarations: function name(args): returnType
        for line in source.lines() {
            let line = line.trim();

            // Skip comments
            if line.starts_with("//") || line.starts_with("/*") {
                continue;
            }

            // Function declarations
            if let Some(func) = parse_function(line) {
                types.insert(func.name.clone(), TypeInfo::Function(func));
            }

            // Type aliases: export type Name = { ... }
            if let Some(struct_info) = parse_type_alias(line, source) {
                types.insert(struct_info.name.clone(), TypeInfo::Struct(struct_info));
            }

            // Enum declarations
            if let Some(enum_info) = parse_enum(line) {
                types.insert(enum_info.name.clone(), TypeInfo::Enum(enum_info));
            }
        }

        // Parse interface definitions (multi-line)
        parse_interfaces(source, types);
    }
}

impl Default for TypeInferrer {
    fn default() -> Self {
        Self::new()
    }
}
