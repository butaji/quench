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
#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::doc_markdown,
    clippy::unnecessary_wraps,
    clippy::use_self,
    clippy::too_many_lines,
    clippy::option_if_let_else,
    clippy::enum_glob_use,
    clippy::match_same_arms,
    clippy::uninlined_format_args,
    clippy::unused_self,
    clippy::self_only_used_in_recursion,
    clippy::redundant_closure,
    clippy::or_fun_call,
    clippy::derivable_impls,
    clippy::double_must_use,
    clippy::unnested_or_patterns,
    clippy::format_push_string,
    clippy::if_not_else,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::used_underscore_binding,
    clippy::missing_const_for_fn,
    dead_code,
    clippy::unnecessary_box_returns,
    clippy::manual_pattern_char_comparison,
    clippy::cast_possible_wrap,
    clippy::manual_contains,
    clippy::redundant_closure_for_method_calls,
    clippy::useless_conversion,
    clippy::missing_panics_doc,
)]

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
#[derive(Error, Debug, Clone)]
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
