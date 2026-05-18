//! # Runie - TypeScript to Rust Compiler Driver
//!
//! A compiler driver that makes `*.r.ts` and `*.r.tsx` valid source files
//! for Rust projects with zero runtime overhead.

#![deny(clippy::all, clippy::pedantic, clippy::nursery)]
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
    unused_lifetimes
)]
#![allow(
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
    clippy::manual_pattern_char_comparison,
    clippy::cast_possible_wrap,
    clippy::manual_contains,
    clippy::redundant_closure_for_method_calls,
    clippy::useless_conversion,
    clippy::missing_panics_doc,
    clippy::missing_const_for_fn,
    unused_extern_crates,
    unused_qualifications
)]

use thiserror::Error;

// Public API - only expose what's needed
pub mod analyzer;
pub mod codegen;
pub mod driver;
pub mod parser;
pub mod reload;

// Shared utilities
pub mod utils;

// Re-exports for convenience
pub use crate::ParseError as ParseErrorType;
pub use crate::RunieError as Error;

/// Result type for Runie operations.
pub type Result<T> = std::result::Result<T, RunieError>;

/// Errors that can occur during Runie compilation.
#[derive(Error, Debug)]
pub enum RunieError {
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

/// Parse errors with location information.
#[derive(Error, Debug, Clone)]
pub enum ParseError {
    #[error("Parse error: {0}")]
    Parse(String),

    #[error("File not found: {0}")]
    NotFound(String),

    #[error("Invalid file extension: {0}")]
    InvalidExtension(String),
}

/// Location in source code with line/column info.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceLocation {
    pub file: String,
    pub line: u32,
    pub column: u32,
}

impl SourceLocation {
    /// Create a new location.
    #[must_use]
    pub fn new(file: impl Into<String>, line: u32, column: u32) -> Self {
        Self {
            file: file.into(),
            line,
            column,
        }
    }
}

impl std::fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.column)
    }
}

/// Verify that a file or directory contains valid *.r.ts* files.
///
/// Returns Ok(()) if all files are valid, Err with details otherwise.
pub fn verify(path: &std::path::Path) -> Result<()> {
    let paths = if path.is_file() {
        vec![path.to_path_buf()]
    } else if path.is_dir() {
        let mut sources = Vec::new();
        scan_for_runie_files(path, &mut sources)?;
        sources
    } else {
        return Err(RunieError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Path not found: {}", path.display()),
        )));
    };
    
    if paths.is_empty() {
        println!("No *.r.ts* files found in {}", path.display());
        return Ok(());
    }
    
    println!("Verifying {} file(s)...\n", paths.len());
    
    let mut errors = Vec::new();
    let mut valid_count = 0;
    
    for file_path in &paths {
        print!("{}: ", file_path.display());
        match verify_file(file_path) {
            Ok(_) => {
                println!("✓ valid");
                valid_count += 1;
            }
            Err(e) => {
                println!("✗ invalid: {}", e);
                errors.push((file_path.clone(), e));
            }
        }
    }
    
    println!("\n--- Summary ---");
    println!("Valid: {}/{}", valid_count, paths.len());
    
    if !errors.is_empty() {
        println!("\nErrors:");
        for (path, err) in &errors {
            println!("  {}: {}", path.display(), err);
        }
        return Err(RunieError::Parse(ParseError::Parse(
            format!("{} file(s) failed verification", errors.len()),
        )));
    }
    
    Ok(())
}

/// Scan directory for *.r.ts* and *.r.tsx* files.
fn scan_for_runie_files(dir: &std::path::Path, results: &mut Vec<std::path::PathBuf>) -> Result<()> {
    for entry in walkdir::WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with(".r.ts") || name.ends_with(".r.tsx") {
                    results.push(path.to_path_buf());
                }
            }
        }
    }
    Ok(())
}

/// Verify a single file.
fn verify_file(path: &std::path::Path) -> std::result::Result<(), String> {
    use crate::analyzer::analyze;
    use crate::parser::{SourceFile, is_runie_file};
    
    // Determine file kind
    let kind = if let Some((_, k)) = is_runie_file(path) {
        k
    } else {
        return Err("Not a *.r.ts* file".to_string());
    };
    
    // Parse the file
    let source = SourceFile::parse(path, kind).map_err(|e| format!("{}", e))?;
    
    // Check for parse errors
    if !source.valid {
        let errs: Vec<String> = source.errors.iter().map(|e| format!("{}", e)).collect();
        return Err(format!("parse errors: {}", errs.join(", ")));
    }
    
    // Analyze for semantic errors
    analyze(&source).map_err(|e| format!("analysis: {}", e))?;
    
    Ok(())
}
