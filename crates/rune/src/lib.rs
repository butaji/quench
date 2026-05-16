//! # Rune - TypeScript to Rust Compiler Driver
//!
//! A compiler driver that makes `*.r.ts` and `*.r.tsx` valid source files
//! for Rust projects with zero runtime overhead.

#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(
    unsafe_code,
    bare_trait_objects,
    exported_private_dependencies,
    keyword_idents,
    macro_use_extern_crate,
    missing_abi,
    non_ascii_idents,
    trivial_casts,
    trivial_numeric_casts,
    unused_extern_crates,
    unused_lifetimes,
    unused_qualifications,
)]
#![allow(clippy::module_name_repetitions)]

pub mod analyzer;
pub mod codegen;
pub mod driver;
pub mod parser;
pub mod reload;

use thiserror::Error;

/// Result type for Rune operations.
pub type Result<T> = std::result::Result<T, RuneError>;

/// Errors that can occur during Rune compilation.
#[derive(Error, Debug)]
pub enum RuneError {
    #[error("Parse error: {0}")]
    Parse(#[from] ParseError),

    #[error("Analysis error at {location}: {message}")]
    Analysis { location: String, message: String },

    #[error("Codegen error: {0}")]
    Codegen(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Cargo error: {0}")]
    Cargo(String),

    #[error("Hot reload error: {0}")]
    Reload(String),
}

/// Parse errors.
#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Parse error: {0}")]
    Parse(String),

    #[error("File not found: {0}")]
    NotFound(String),

    #[error("Invalid file extension: {0}")]
    InvalidExtension(String),
}

/// Location in source code.
#[derive(Debug, Clone, Default)]
pub struct SourceLocation {
    pub file: String,
    pub line: u32,
    pub column: u32,
}

impl std::fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.column)
    }
}
