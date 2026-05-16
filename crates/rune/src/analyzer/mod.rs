//! # Analyzer Module
//!
//! Validates the zero-overhead TypeScript subset and performs
//! ownership inference (borrow checking).

mod validator;
mod ownership;
mod context;
mod inference;

pub use validator::{SubsetValidator, ValidationError};
pub use inference::TypeInferrer;
pub use ownership::{OwnershipAnalyzer, BorrowMode};
pub use context::AnalysisContext;

/// Full analysis result for a source file.
#[derive(Debug, Clone)]
pub struct AnalysisResult {
    /// Validated types
    pub types: TypeMap,
    /// Ownership analysis per binding
    pub ownership: OwnershipAnalysis,
    /// Warnings generated during analysis
    pub warnings: Vec<AnalysisWarning>,
}

/// Type mapping for a file.
#[derive(Debug, Clone, Default)]
pub struct TypeMap {
    entries: std::collections::HashMap<String, TypeInfo>,
}

impl TypeMap {
    /// Insert a type for a binding.
    pub fn insert(&mut self, name: String, info: TypeInfo) {
        self.entries.insert(name, info);
    }

    /// Get type info for a binding.
    pub fn get(&self, name: &str) -> Option<&TypeInfo> {
        self.entries.get(name)
    }

    /// Iterate over all entries.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &TypeInfo)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
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
    /// Unknown type (error)
    Unknown,
}

/// Struct type information.
#[derive(Debug, Clone)]
pub struct StructInfo {
    pub name: String,
    pub fields: Vec<(String, TypeInfo)>,
}

/// Enum type information (tagged union).
#[derive(Debug, Clone)]
pub struct EnumInfo {
    pub name: String,
    pub variants: Vec<EnumVariant>,
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
    pub params: Vec<(String, TypeInfo)>,
    pub return_type: TypeInfo,
    pub is_async: bool,
}

/// Ownership analysis per binding.
#[derive(Debug, Clone, Default)]
pub struct OwnershipAnalysis {
    bindings: std::collections::HashMap<String, BorrowMode>,
}

impl OwnershipAnalysis {
    /// Record borrow mode for a binding.
    pub fn set(&mut self, name: String, mode: BorrowMode) {
        self.bindings.insert(name, mode);
    }

    /// Get borrow mode for a binding.
    pub fn get(&self, name: &str) -> Option<BorrowMode> {
        self.bindings.get(name).copied()
    }
}

/// Warnings generated during analysis.
#[derive(Debug, Clone)]
pub struct AnalysisWarning {
    pub location: String,
    pub message: String,
    pub code: &'static str,
}

/// Analyze a source file.
pub fn analyze(source: &crate::parser::SourceFile) -> crate::Result<AnalysisResult> {
    let mut ctx = AnalysisContext::new(source);
    let validator = SubsetValidator::new();
    let type_inferrer = TypeInferrer::new();
    let ownership_analyzer = OwnershipAnalyzer::new();

    validator.validate_module(&source.module, &mut ctx)?;
    let types = type_inferrer.infer_types(&source.module, &ctx)?;
    let ownership = ownership_analyzer.analyze(&source.module, &ctx)?;
    let warnings = ctx.take_warnings();

    Ok(AnalysisResult {
        types,
        ownership,
        warnings,
    })
}
