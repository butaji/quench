//! # Code Generator
//!
//! Transpiles TypeScript AST to Rust source code.

mod emitter;
mod types;
mod jsx;

pub use emitter::{RustEmitter, EmitOptions};
pub use jsx::JsxTranspiler;

use crate::{parser::SourceFile, analyzer::AnalysisResult};

/// Options for code generation.
#[derive(Debug, Clone, Default)]
pub struct CodegenOptions {
    /// Generate debug info
    pub debug: bool,
    /// Target Rust edition
    pub edition: String,
    /// Enable hot reload mode
    pub hot_reload: bool,
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
    /// Type definitions
    pub types: Vec<String>,
    /// Function definitions
    pub functions: Vec<String>,
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

impl Import {
    /// Convert to Rust import statement.
    #[must_use]
    pub fn to_rust(&self) -> String {
        if self.is_native {
            let path = self.path.replace(':', "::");
            format!("use crate::native::{path};")
        } else {
            let names: Vec<_> = self.names.iter()
                .map(|n| n.rust_name.clone())
                .collect();
            let clean_path = self.path
                .replace(".r.ts", "")
                .replace(".r.tsx", "");
            format!("use {clean_path}::{{{}}};", names.join(", "))
        }
    }
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
///
/// # Errors
/// Returns an error if code generation fails.
pub fn generate(
    source: &SourceFile,
    analysis: &AnalysisResult,
) -> crate::Result<GeneratedModule> {
    let emitter = RustEmitter::new(source, analysis);
    emitter.emit()
}
