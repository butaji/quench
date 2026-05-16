//! # Code Generator
//!
//! Transpiles TypeScript AST to Rust source code.
//! Handles type mapping, JSX transformation, and code emission.

mod emitter;
mod types;
mod jsx;
mod emitters;

pub use emitter::{RustEmitter, EmitOptions};
pub use types::TypeEmitter;
pub use jsx::JsxTranspiler;
pub use emitters::{ExprEmitter, StmtEmitter};

/// Options for code generation.
#[derive(Debug, Clone)]
pub struct CodegenOptions {
    /// Generate debug info
    pub debug: bool,
    /// Target Rust edition
    pub edition: String,
    /// Enable hot reload mode
    pub hot_reload: bool,
}

impl Default for CodegenOptions {
    fn default() -> Self {
        Self {
            debug: false,
            edition: "2021".to_string(),
            hot_reload: false,
        }
    }
}

/// Generated Rust module.
#[derive(Debug, Clone)]
pub struct GeneratedModule {
    /// Module name (file stem)
    pub name: String,
    /// Generated Rust source code
    pub source: String,
    /// Imports required by this module
    pub imports: Vec<Import>,
}

/// An import statement.
#[derive(Debug, Clone)]
pub struct Import {
    /// Import path
    pub path: String,
    /// Imported names
    pub names: Vec<ImportedName>,
    /// Is this a native import?
    pub is_native: bool,
}

/// A single imported name.
#[derive(Debug, Clone)]
pub struct ImportedName {
    /// Original name in TypeScript
    pub original: String,
    /// Name to use in Rust
    pub rust_name: String,
}

/// Generate Rust code from analyzed source.
pub fn generate(
    source: &crate::parser::SourceFile,
    analysis: &crate::analyzer::AnalysisResult,
) -> crate::Result<GeneratedModule> {
    let emitter = RustEmitter::new(source, analysis);
    emitter.emit()
}
