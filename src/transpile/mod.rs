//! Transpilation pipeline for TS/TSX to Rust

pub mod analyzer;
pub mod errors;
pub mod hir;
pub mod js_bundle;
pub mod js_codegen;

pub mod parallel;
pub mod parser;


#[cfg(test)]
mod tests;

#[cfg(test)]
mod js_codegen_tests;

#[cfg(test)]
mod runtime_tests;

pub use parser::TsParser;


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
