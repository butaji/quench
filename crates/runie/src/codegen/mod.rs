//! # Code Generator
//!
//! Transpiles TypeScript AST to Rust source code.

pub mod emitter;
mod jsx;
mod types;

pub use emitter::{EmitOptions, RustEmitter};
pub use jsx::JsxTranspiler;
pub use types::{EnumDefinition, EnumVariant, RustType, StructFields};

use crate::{analyzer::AnalysisResult, parser::SourceFile};

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
            let names: Vec<_> = self.names.iter().map(|n| n.rust_name.clone()).collect();
            let clean_path = self.path.replace(".r.ts", "").replace(".r.tsx", "");
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
pub fn generate(source: &SourceFile, analysis: &AnalysisResult) -> crate::Result<GeneratedModule> {
    let emitter = RustEmitter::new(source, analysis);
    emitter.emit(source)
}

#[cfg(test)]
mod comprehensive_tests;
#[cfg(test)]
mod example_validation_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_struct() {
        let source = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: crate::parser::SourceKind::TypeScript,
            source: String::from(
                "export type Task = { id: number, title: string, done: boolean };",
            ),
            name: String::from("test"),
            valid: true,
            errors: Vec::new(),
        };

        let analysis = AnalysisResult::default();
        let result = generate(&source, &analysis);

        assert!(result.is_ok());
        let module = result.unwrap();
        assert!(module.source.contains("pub struct Task"));
        assert!(module.source.contains("pub id: f64"));
        assert!(module.source.contains("pub title: String"));
        assert!(module.source.contains("pub done: bool"));
    }

    #[test]
    fn test_generate_enum() {
        let source = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: crate::parser::SourceKind::TypeScript,
            source: String::from("export enum Filter { All, Active, Completed }"),
            name: String::from("test"),
            valid: true,
            errors: Vec::new(),
        };

        let analysis = AnalysisResult::default();
        let result = generate(&source, &analysis);

        assert!(result.is_ok());
        let module = result.unwrap();
        assert!(module.source.contains("pub enum Filter"));
        assert!(module.source.contains("All"));
        assert!(module.source.contains("Active"));
        assert!(module.source.contains("Completed"));
    }

    #[test]
    fn test_generate_function() {
        let source = SourceFile {
            path: std::path::PathBuf::from("test.r.ts"),
            kind: crate::parser::SourceKind::TypeScript,
            source: String::from(
                "export function add(a: number, b: number): number { return a + b; }",
            ),
            name: String::from("test"),
            valid: true,
            errors: Vec::new(),
        };

        let analysis = AnalysisResult::default();
        let result = generate(&source, &analysis);

        assert!(result.is_ok());
        let module = result.unwrap();
        assert!(module.source.contains("pub fn add"));
    }
}
