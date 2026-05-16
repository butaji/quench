//! # Type Inference Module
//!
//! Infers Rust types from TypeScript source.

mod primitives;
mod ts_types;

use crate::parser::SourceFile;
use crate::analyzer::{TypeMap, TypeInfo, FunctionInfo, StructInfo, EnumInfo};

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
    #[must_use]
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
            if let Some(func) = self.parse_function(line) {
                types.insert(func.name.clone(), TypeInfo::Function(func));
            }

            // Type aliases: export type Name = { ... }
            if let Some(struct_info) = self.parse_type_alias(line, source) {
                types.insert(struct_info.name.clone(), TypeInfo::Struct(struct_info));
            }

            // Enum declarations
            if let Some(enum_info) = self.parse_enum(line) {
                types.insert(enum_info.name.clone(), TypeInfo::Enum(enum_info));
            }
        }

        // Parse interface definitions (multi-line)
        self.parse_interfaces(source, types);
    }

    /// Parse a function declaration from a line.
    fn parse_function(&self, line: &str) -> Option<FunctionInfo> {
        // Pattern: export function name(args): returnType
        // or: function name(args): returnType
        let pattern = if line.starts_with("export") {
            "export function "
        } else {
            "function "
        };

        let rest = line.strip_prefix(pattern)?;
        let rest = rest.strip_prefix("async ").unwrap_or(rest);
        let rest = rest.strip_prefix("pub ").unwrap_or(rest);

        // Extract function name
        let name_end = rest.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(rest.len());
        let name = rest[..name_end].to_string();
        let rest = &rest[name_end..];

        // Extract parameters
        let (params_str, rest) = self.extract_in_parens(rest)?;

        // Extract return type
        let return_type = if let Some(ret) = rest.strip_prefix("): ") {
            self.parse_type(ret.trim_end_matches(';'))
        } else {
            Box::new(TypeInfo::Unknown)
        };

        let params = self.parse_params(&params_str);

        Some(FunctionInfo {
            name,
            params,
            return_type,
            is_async: line.contains("async function"),
            is_method: false,
        })
    }

    /// Extract content within parentheses.
    fn extract_in_parens<'a>(&self, s: &'a str) -> Option<(String, &'a str)> {
        let mut depth = 0;
        let mut start = None;

        for (i, c) in s.char_indices() {
            match c {
                '(' if depth == 0 => start = Some(i + 1),
                '(' => depth += 1,
                ')' if depth == 0 => {
                    let end = i;
                    let content = &s[start?..end];
                    let rest = &s[i + 1..];
                    return Some((content.to_string(), rest.trim_start()));
                }
                ')' => depth -= 1,
                _ => {}
            }
        }
        None
    }

    /// Parse parameter list.
    fn parse_params(&self, params_str: &str) -> Vec<(String, TypeInfo)> {
        let mut params = Vec::new();

        for param in params_str.split(',') {
            let param = param.trim();
            if param.is_empty() {
                continue;
            }

            // Pattern: name: type
            if let Some((idx, _)) = param.match_indices(": ").next() {
                let name = param[..idx].trim().to_string();
                let type_str = param[idx + 2..].trim();
                let type_info = *self.parse_type(type_str);
                params.push((name, type_info));
            } else if let Some((idx, _)) = param.match_indices(':').next() {
                let name = param[..idx].trim().to_string();
                let type_str = param[idx + 1..].trim();
                let type_info = *self.parse_type(type_str);
                params.push((name, type_info));
            } else {
                // Infer from name or default
                params.push((param.to_string(), TypeInfo::Unknown));
            }
        }

        params
    }

    /// Parse a type string.
    fn parse_type(&self, type_str: &str) -> Box<TypeInfo> {
        let s = type_str.trim();

        // Remove trailing punctuation
        let s = s.trim_end_matches(|c: char| c == ';' || c == ',' || c == ')' || c == '>');

        // Void
        if s == "void" || s == "undefined" {
            return Box::new(TypeInfo::Unknown);
        }

        // Primitive types
        match s {
            "number" => return Box::new(TypeInfo::Float),
            "string" => return Box::new(TypeInfo::String),
            "boolean" => return Box::new(TypeInfo::Boolean),
            "null" => return Box::new(TypeInfo::Unknown),
            _ => {}
        }

        // String literals
        if (s.starts_with('"') && s.ends_with('"'))
            || (s.starts_with('\'') && s.ends_with('\'')) {
            return Box::new(TypeInfo::StringLiteral(s[1..s.len() - 1].to_string()));
        }

        // Integer literals
        if let Ok(n) = s.parse::<i64>() {
            return Box::new(TypeInfo::Integer(n));
        }

        // Array type: T[]
        if let Some(inner) = s.strip_suffix("[]") {
            return Box::new(TypeInfo::Array(self.parse_type(inner)));
        }

        // Option type: T | null
        if let Some(inner) = s.strip_suffix(" | null") {
            return Box::new(TypeInfo::Option(self.parse_type(inner)));
        }

        // Result type patterns
        if s.contains("ok:") && s.contains("error:") {
            return Box::new(TypeInfo::Result(
                Box::new(TypeInfo::Unknown),
                Box::new(TypeInfo::String),
            ));
        }

        // Generic type: Name<T>
        if let Some((name, generic)) = s.split_once('<') {
            let generic = generic.trim_end_matches('>');
            if generic.contains(',') {
                // Multiple type params - just use first
                let first = generic.split(',').next().unwrap_or("T");
                return Box::new(TypeInfo::Generic(format!("{}<{}>", name, first)));
            }
            return Box::new(TypeInfo::Generic(format!("{}<{}>", name, generic)));
        }

        // Regular type name
        Box::new(TypeInfo::Struct(StructInfo {
            name: s.to_string(),
            fields: Vec::new(),
        }))
    }

    /// Parse a type alias or interface.
    fn parse_type_alias(&self, line: &str, _source: &str) -> Option<StructInfo> {
        // Pattern: export type Name = { ... }
        let prefix = if line.starts_with("export") {
            line.strip_prefix("export ")?
        } else {
            line
        };

        let prefix = prefix.strip_prefix("type ")?;
        let name_end = prefix.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(prefix.len());
        let name = prefix[..name_end].to_string();

        let rest = &prefix[name_end..];

        if !rest.trim().starts_with('=') {
            return None;
        }

        let rest = rest.trim_start_matches('=').trim();

        // This is an alias to another type
        if !rest.starts_with('{') {
            return Some(StructInfo {
                name,
                fields: Vec::new(),
            });
        }

        None
    }

    /// Parse interface definitions (multi-line).
    fn parse_interfaces(&self, source: &str, types: &mut TypeMap) {
        let mut in_interface = false;
        let mut current_name = String::new();
        let mut brace_depth = 0;
        let mut fields = Vec::new();

        for line in source.lines() {
            let line = line.trim();

            // Start of interface
            if line.starts_with("export type ")
                && (line.contains("= {") || line.contains(" ={")) {
                in_interface = true;
                let name_part = line.strip_prefix("export type ").unwrap();
                let name_end = name_part.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(name_part.len());
                current_name = name_part[..name_end].to_string();

                brace_depth = line.matches('{').count() - line.matches('}').count();

                // Extract fields from same line if present
                if let Some(start) = line.find('{') {
                    let end = line.rfind('}');
                    if let Some(end) = end {
                        let body = &line[start + 1..end];
                        for field in body.split(';') {
                            if let Some(f) = self.parse_field(field) {
                                fields.push(f);
                            }
                        }
                    }
                }
                continue;
            }

            if in_interface {
                brace_depth = (brace_depth as i32 + line.matches('{').count() as i32 - line.matches('}').count() as i32) as usize;

                if brace_depth == 0 {
                    // End of interface
                    types.insert(
                        current_name.clone(),
                        TypeInfo::Struct(StructInfo {
                            name: current_name.clone(),
                            fields: fields.clone(),
                        }),
                    );
                    in_interface = false;
                    fields.clear();
                    current_name.clear();
                } else if let Some(field) = self.parse_field(line) {
                    fields.push(field);
                }
            }
        }
    }

    /// Parse a single field from a line.
    fn parse_field(&self, line: &str) -> Option<(String, TypeInfo)> {
        let line = line.trim().trim_end_matches(',').trim_end_matches(';');

        if line.is_empty() || line.starts_with("//") {
            return None;
        }

        // Pattern: name: type
        if let Some((idx, _)) = line.match_indices(": ").next() {
            let name = line[..idx].trim().to_string();
            let type_str = line[idx + 2..].trim();
            let type_info = self.parse_type(type_str);
            return Some((name, *type_info));
        }

        // Pattern: name:type (no space)
        if let Some((idx, _)) = line.match_indices(':').next() {
            let name = line[..idx].trim().to_string();
            let type_str = line[idx + 1..].trim();
            let type_info = self.parse_type(type_str);
            return Some((name, *type_info));
        }

        None
    }

    /// Parse an enum declaration.
    fn parse_enum(&self, line: &str) -> Option<EnumInfo> {
        // Pattern: export enum Name
        let prefix = if line.starts_with("export ") {
            line.strip_prefix("export ")?
        } else {
            line
        };

        if !prefix.starts_with("enum ") {
            return None;
        }

        let rest = prefix.strip_prefix("enum ").unwrap();
        let name_end = rest.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(rest.len());
        let name = rest[..name_end].to_string();

        Some(EnumInfo {
            name,
            variants: Vec::new(),
        })
    }
}

impl Default for TypeInferrer {
    fn default() -> Self {
        Self::new()
    }
}
