//! Transpilation pipeline for TS/TSX to Rust

pub mod parser;
pub mod analyzer;
pub mod codegen;
pub mod hir;
pub mod jsx_transformer;
pub mod routegen;
pub mod middlewaregen;
pub mod errors;
pub mod js_codegen;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod js_codegen_tests;

pub use crate::config::Config;
pub use parser::Parser;
pub use analyzer::Analyzer;
pub use codegen::CodeGenerator;

use anyhow::{Context, Result};
use std::path::PathBuf;

/// Transpilation result
#[allow(dead_code)]
pub struct TranspileResult {
    /// Generated Rust code
    pub rust_code: String,
    /// Source map (for debugging)
    pub source_map: Option<String>,
    /// Warnings
    pub warnings: Vec<String>,
}

/// Transpilation error
#[allow(dead_code)]
#[derive(Debug, thiserror::Error)]
pub enum TranspileError {
    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Type error: {0}")]
    Type(String),

    #[error("Unsupported feature: {0}")]
    Unsupported(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Main transpiler
pub struct Transpiler {
    #[allow(dead_code)]
    config: Config,
    parser: Parser,
    analyzer: Analyzer,
    codegen: CodeGenerator,
}

impl Transpiler {
    pub fn new(config: &Config) -> Self {
        Self {
            config: config.clone(),
            parser: Parser::new(),
            analyzer: Analyzer::new(),
            codegen: CodeGenerator::new(),
        }
    }

    /// Parse a TypeScript file
    pub fn parse_file(&mut self, path: &PathBuf) -> Result<hir::Module> {
        self.parser.parse_file(path)
    }

    /// Analyze a parsed module
    #[allow(dead_code)]
    pub fn analyze(&mut self, module: &hir::Module) -> Result<(), Vec<TranspileError>> {
        self.analyzer.analyze(module).map_err(|errors| {
            errors.into_iter().map(|e| TranspileError::Type(e.to_string())).collect()
        })
    }

    /// Transpile a single file
    pub fn transpile_file(&mut self, path: &PathBuf) -> Result<String> {
        // Parse
        let module = self.parser.parse_file(path)
            .context("Failed to parse file")?;

        // Analyze
        if let Err(errs) = self.analyzer.analyze(&module) {
            anyhow::bail!("Analysis failed: {:?}", errs);
        }

        // Generate
        let rust_code = self.codegen.generate_module(&module)
            .map_err(|e| anyhow::anyhow!("Failed to generate Rust code: {}", e))?;

        Ok(rust_code)
    }

    /// Transpile multiple files
    #[allow(dead_code)]
    pub fn transpile_files(&mut self, paths: &[PathBuf]) -> Result<Vec<(PathBuf, String)>> {
        let mut results = Vec::new();

        for path in paths {
            match self.transpile_file(path) {
                Ok(code) => results.push((path.clone(), code)),
                Err(e) => {
                    eprintln!("Error transpiling {:?}: {}", path, e);
                }
            }
        }

        Ok(results)
    }
}
