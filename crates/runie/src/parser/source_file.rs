//! # Source File Handling
//!
//! Manages source file parsing with SWC.

use crate::parser::swc_parser::SwcAst;
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
        let file_name = path.to_string_lossy();
        Self::parse_and_collect_errors(source, &file_name, kind)
    }

    /// Parse and collect any errors.
    fn parse_and_collect_errors(
        source: &str,
        file_name: &str,
        kind: SourceKind,
    ) -> Vec<ParseError> {
        let parse_result = Self::try_parse(source, file_name, kind);
        match parse_result {
            Ok(()) => Vec::new(),
            Err(e) => Self::single_error(e),
        }
    }

    /// Try to parse source.
    fn try_parse(
        source: &str,
        file_name: &str,
        kind: SourceKind,
    ) -> std::result::Result<(), String> {
        match kind {
            SourceKind::TypeScript => SwcAst::parse_ts(source, file_name).map(|_| ()),
            SourceKind::Tsx => SwcAst::parse_tsx(source, file_name).map(|_| ()),
        }
        .map_err(|e| format!("{:?}", e))
    }

    /// Create a single error vec.
    fn single_error(msg: String) -> Vec<ParseError> {
        vec![ParseError::Parse(msg)]
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

/// Parse source from a string (for testing).
///
/// # Errors
/// Returns an error if the source cannot be parsed.
#[must_use]
pub fn parse_file_from_str(source: &str, name: &str) -> Result<SourceFile> {
    let is_tsx = std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("tsx"));
    let kind = if is_tsx || name.ends_with(".r.tsx") {
        SourceKind::Tsx
    } else {
        SourceKind::TypeScript
    };

    let file_name = if name.contains('.') {
        name.to_string()
    } else {
        format!("{}.r.ts", name)
    };

    let errors = SourceFile::parse_and_collect_errors(source, &file_name, kind);

    Ok(SourceFile {
        path: PathBuf::from(&file_name),
        kind,
        source: source.to_string(),
        name: name.to_string(),
        valid: errors.is_empty(),
        errors,
    })
}
