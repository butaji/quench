//! # Analyzer Module
//!
//! Validates the zero-overhead TypeScript subset and performs
//! ownership inference (borrow checking).

mod ownership;
mod context;
mod inference;
mod validator;

pub use ownership::{OwnershipAnalyzer, BorrowMode};
pub use context::AnalysisContext;
pub use inference::TypeInferrer;
pub use validator::SubsetValidator;

use std::collections::HashMap;

/// Full analysis result for a source file.
#[derive(Debug, Clone, Default)]
pub struct AnalysisResult {
    /// Inferred types
    pub types: TypeMap,
    /// Ownership analysis per binding
    pub ownership: OwnershipAnalysis,
    /// Warnings generated during analysis
    pub warnings: Vec<AnalysisWarning>,
    /// Exported functions
    pub exports: Vec<ExportInfo>,
    /// Imported modules
    pub imports: Vec<ImportInfo>,
}

/// Type mapping for a file.
#[derive(Debug, Clone, Default)]
pub struct TypeMap {
    entries: HashMap<String, TypeInfo>,
}

impl TypeMap {
    /// Insert a type for a binding.
    pub fn insert(&mut self, name: String, info: TypeInfo) {
        self.entries.insert(name, info);
    }

    /// Get type info for a binding.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&TypeInfo> {
        self.entries.get(name)
    }

    /// Iterate over all entries.
    #[must_use]
    pub fn iter(&self) -> impl Iterator<Item = (&str, &TypeInfo)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Get mutable reference for building.
    pub fn entries_mut(&mut self) -> &mut HashMap<String, TypeInfo> {
        &mut self.entries
    }
}

/// Type information inferred from TypeScript.
#[derive(Debug, Clone)]
pub enum TypeInfo {
    /// Integer literal
    Integer(i64),
    /// Floating point
    Float,
    /// String type
    String,
    /// String literal
    StringLiteral(String),
    /// Boolean
    Boolean,
    /// Array type
    Array(Box<TypeInfo>),
    /// Struct type
    Struct(StructInfo),
    /// Enum type (tagged union)
    Enum(EnumInfo),
    /// Option type
    Option(Box<TypeInfo>),
    /// Result type
    Result(Box<TypeInfo>, Box<TypeInfo>),
    /// Function type
    Function(FunctionInfo),
    /// Generic type parameter
    Generic(String),
    /// Unknown type (error)
    Unknown,
}

impl TypeInfo {
    /// Get the Rust type representation.
    #[must_use]
    pub fn to_rust_type(&self) -> String {
        match self {
            TypeInfo::Integer(_) => "i32".to_string(),
            TypeInfo::Float => "f64".to_string(),
            TypeInfo::String => "String".to_string(),
            TypeInfo::StringLiteral(_) => "&str".to_string(),
            TypeInfo::Boolean => "bool".to_string(),
            TypeInfo::Array(inner) => format!("Vec<{}>", inner.to_rust_type()),
            TypeInfo::Struct(s) => s.name.clone(),
            TypeInfo::Enum(e) => e.name.clone(),
            TypeInfo::Option(inner) => format!("Option<{}>", inner.to_rust_type()),
            TypeInfo::Result(ok, _) => format!("Result<{}, String>", ok.to_rust_type()),
            TypeInfo::Function(f) => f.to_rust_signature(),
            TypeInfo::Generic(name) => name.clone(),
            TypeInfo::Unknown => "()".to_string(),
        }
    }

    /// Check if this is an integer type.
    #[must_use]
    pub fn is_integer(&self) -> bool {
        matches!(self, TypeInfo::Integer(_))
    }

    /// Check if this is a float type.
    #[must_use]
    pub fn is_float(&self) -> bool {
        matches!(self, TypeInfo::Float)
    }
}

/// Struct type information.
#[derive(Debug, Clone)]
pub struct StructInfo {
    pub name: String,
    pub fields: Vec<(String, TypeInfo)>,
}

impl StructInfo {
    /// Generate Rust struct definition.
    #[must_use]
    pub fn to_rust(&self) -> String {
        let fields = self
            .fields
            .iter()
            .map(|(name, ty)| {
                format!("    pub {}: {},", Self::to_snake_case(name), ty.to_rust_type())
            })
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "#[derive(Clone, Debug)]\npub struct {} {{\n{}\n}}",
            Self::to_pascal_case(&self.name),
            fields
        )
    }

    fn to_snake_case(s: &str) -> String {
        let mut result = String::new();
        for (i, c) in s.chars().enumerate() {
            if c.is_uppercase() && i > 0 {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        }
        result
    }

    fn to_pascal_case(s: &str) -> String {
        let mut result = String::new();
        let mut capitalize_next = true;
        for c in s.chars() {
            if c == '_' || c == '-' {
                capitalize_next = true;
            } else if capitalize_next {
                result.push(c.to_ascii_uppercase());
                capitalize_next = false;
            } else {
                result.push(c);
            }
        }
        result
    }
}

/// Enum type information (tagged union).
#[derive(Debug, Clone)]
pub struct EnumInfo {
    pub name: String,
    pub variants: Vec<EnumVariant>,
}

impl EnumInfo {
    /// Generate Rust enum definition.
    #[must_use]
    pub fn to_rust(&self) -> String {
        let variants = self
            .variants
            .iter()
            .map(|v| {
                if v.fields.is_empty() {
                    format!("    {},", Self::to_pascal_case(&v.tag))
                } else {
                    let fields = v
                        .fields
                        .iter()
                        .map(|(n, t)| format!("{}: {}", Self::to_snake_case(n), t.to_rust_type()))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("    {} {{ {} }},", Self::to_pascal_case(&v.tag), fields)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "#[derive(Clone, Debug)]\npub enum {} {{\n{}\n}}",
            Self::to_pascal_case(&self.name),
            variants
        )
    }

    fn to_snake_case(s: &str) -> String {
        let mut result = String::new();
        for (i, c) in s.chars().enumerate() {
            if c.is_uppercase() && i > 0 {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        }
        result
    }

    fn to_pascal_case(s: &str) -> String {
        let mut result = String::new();
        let mut capitalize_next = true;
        for c in s.chars() {
            if c == '_' || c == '-' {
                capitalize_next = true;
            } else if capitalize_next {
                result.push(c.to_ascii_uppercase());
                capitalize_next = false;
            } else {
                result.push(c);
            }
        }
        result
    }
}

/// Enum variant.
#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub tag: String,
    pub fields: Vec<(String, TypeInfo)>,
}

/// Function type information.
#[derive(Debug, Clone)]
pub struct FunctionInfo {
    pub name: String,
    pub params: Vec<(String, TypeInfo)>,
    pub return_type: Box<TypeInfo>,
    pub is_async: bool,
    pub is_method: bool,
}

impl FunctionInfo {
    /// Generate Rust function signature.
    #[must_use]
    pub fn to_rust_signature(&self) -> String {
        let params = self
            .params
            .iter()
            .map(|(n, t)| format!("{}: {}", Self::to_snake_case(n), t.to_rust_type()))
            .collect::<Vec<_>>()
            .join(", ");

        let async_prefix = if self.is_async { "async " } else { "" };

        format!(
            "{}fn({params}) -> {}",
            async_prefix,
            self.return_type.to_rust_type()
        )
    }

    fn to_snake_case(s: &str) -> String {
        let mut result = String::new();
        for (i, c) in s.chars().enumerate() {
            if c.is_uppercase() && i > 0 {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        }
        result
    }
}

/// Ownership analysis per binding.
#[derive(Debug, Clone, Default)]
pub struct OwnershipAnalysis {
    bindings: HashMap<String, BorrowMode>,
}

impl OwnershipAnalysis {
    /// Record borrow mode for a binding.
    pub fn set(&mut self, name: String, mode: BorrowMode) {
        self.bindings.insert(name, mode);
    }

    /// Get borrow mode for a binding.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<BorrowMode> {
        self.bindings.get(name).copied()
    }

    /// Get all bindings.
    #[must_use]
    pub const fn bindings(&self) -> &HashMap<String, BorrowMode> {
        &self.bindings
    }
}

/// Warnings generated during analysis.
#[derive(Debug, Clone)]
pub struct AnalysisWarning {
    pub location: String,
    pub message: String,
    pub code: &'static str,
}

/// Export information.
#[derive(Debug, Clone)]
pub struct ExportInfo {
    pub name: String,
    pub rust_name: String,
    pub type_info: TypeInfo,
}

/// Import information.
#[derive(Debug, Clone)]
pub struct ImportInfo {
    pub path: String,
    pub names: Vec<String>,
    pub is_native: bool,
}

/// Analyze a source file.
#[allow(clippy::too_many_lines)]
pub fn analyze(source: &crate::parser::SourceFile) -> crate::Result<AnalysisResult> {
    let mut ctx = AnalysisContext::new(source);
    let mut type_inferrer = TypeInferrer::new();
    let mut ownership_analyzer = OwnershipAnalyzer::new();
    let validator = SubsetValidator::new();

    // Check for parse errors first
    for err in &source.errors {
        ctx.add_warning(
            source.path.display().to_string(),
            format!("Parse error: {err}"),
            "parse_error",
        );
    }

    // Validate the subset
    if let Err(e) = validator.validate(source) {
        ctx.add_warning(
            e.location,
            e.message,
            e.code,
        );
    }

    // Infer types from source
    let types = type_inferrer.infer_from_source(source)?;

    // Analyze ownership
    let ownership = ownership_analyzer.analyze(&types);

    // Build exports from types
    let exports = types
        .iter()
        .filter(|(_, info)| matches!(info, TypeInfo::Function(_)))
        .map(|(name, info)| {
            let rust_name = to_snake_case(name);
            ExportInfo {
                name: name.to_string(),
                rust_name,
                type_info: info.clone(),
            }
        })
        .collect();

    Ok(AnalysisResult {
        types,
        ownership,
        warnings: ctx.take_warnings(),
        exports,
        imports: Vec::new(),
    })
}

fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(c.to_ascii_lowercase());
    }
    result
}
