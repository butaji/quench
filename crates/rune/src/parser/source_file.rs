//! # Source File Handling
//!
//! Manages source file parsing with SWC.

use crate::{ParseError, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Kind of source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// Standard TypeScript file (.r.ts)
    TypeScript,
    /// TSX file (.r.tsx)
    Tsx,
}

/// A parsed Rune source file.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// File path
    pub path: PathBuf,
    /// Kind of source file
    pub kind: SourceKind,
    /// Raw source text
    pub source: String,
    /// Module name for display
    pub name: String,
    /// Whether parsing was successful
    pub valid: bool,
    /// Parse errors if any
    pub errors: Vec<ParseError>,
}

impl SourceFile {
    /// Parse a source file from a path.
    ///
    /// # Errors
    /// Returns an error if the file cannot be read.
    pub fn parse(path: &Path, kind: SourceKind) -> Result<Self> {
        if !path.exists() {
            return Err(ParseError::NotFound(path.display().to_string()).into());
        }

        let source = fs::read_to_string(path)?;

        let name = path
            .file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("module")
            .to_string();

        let errors = Self::validate_with_swc(&source, path, kind);

        Ok(Self {
            path: path.to_path_buf(),
            kind,
            source,
            name,
            valid: errors.is_empty(),
            errors,
        })
    }

    /// Validate source using SWC.
    fn validate_with_swc(source: &str, path: &Path, kind: SourceKind) -> Vec<ParseError> {
        use crate::parser::swc_parser::SwcAst;

        let file_name = path.to_string_lossy();

        let result = match kind {
            SourceKind::TypeScript => SwcAst::parse_ts(source, &file_name),
            SourceKind::Tsx => SwcAst::parse_tsx(source, &file_name),
        };

        match result {
            Ok(_ast) => Vec::new(),
            Err(e) => vec![ParseError::Parse(format!("{:?}", e))],
        }
    }

    /// Get line and column from byte offset.
    #[must_use]
    pub fn location_from_offset(&self, offset: u32) -> (u32, u32) {
        let mut line = 1u32;
        let mut col = 1u32;

        for (pos, c) in self.source.chars().enumerate() {
            if pos as u32 >= offset {
                break;
            }
            if c == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }

        (line, col)
    }

    /// Get the module name.
    #[must_use]
    pub fn module_name(&self) -> &str {
        &self.name
    }

    /// Check if this is a TSX file.
    #[must_use]
    pub fn is_tsx(&self) -> bool {
        self.kind == SourceKind::Tsx
    }
}
