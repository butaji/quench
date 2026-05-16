//! # Source File Handling
//!
//! Manages source file parsing.

use std::fs;
use std::path::{Path, PathBuf};
use crate::{ParseError, Result};

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

        let errors = Self::validate_syntax(&source);

        Ok(Self {
            path: path.to_path_buf(),
            kind,
            source,
            name,
            valid: errors.is_empty(),
            errors,
        })
    }

    /// Validate basic syntax.
    fn validate_syntax(source: &str) -> Vec<ParseError> {
        let mut errors = Vec::new();
        let mut depth = SyntaxDepth::default();

        let mut in_string = false;
        let mut string_char = '"';
        let mut line = 1u32;

        let chars: Vec<char> = source.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let c = chars[i];

            if c == '\n' {
                line += 1;
            }

            // Handle string literals
            if !in_string && (c == '"' || c == '\'') {
                in_string = true;
                string_char = c;
            } else if in_string && c == string_char && !escaped(&chars, i) {
                in_string = false;
            } else if !in_string {
                match c {
                    '{' => depth.brace += 1,
                    '}' => {
                        depth.brace -= 1;
                        if depth.brace < 0 {
                            errors.push(ParseError::Parse(format!(
                                "Unexpected closing brace at line {line}"
                            )));
                            depth.brace = 0;
                        }
                    }
                    '(' => depth.paren += 1,
                    ')' => {
                        depth.paren -= 1;
                        if depth.paren < 0 {
                            errors.push(ParseError::Parse(format!(
                                "Unexpected closing parenthesis at line {line}"
                            )));
                            depth.paren = 0;
                        }
                    }
                    '[' => depth.bracket += 1,
                    ']' => {
                        depth.bracket -= 1;
                        if depth.bracket < 0 {
                            errors.push(ParseError::Parse(format!(
                                "Unexpected closing bracket at line {line}"
                            )));
                            depth.bracket = 0;
                        }
                    }
                    _ => {}
                }
            }

            i += 1;
        }

        if depth.brace != 0 {
            errors.push(ParseError::Parse("Unclosed brace(s) at end of file".into()));
        }
        if depth.paren != 0 {
            errors.push(ParseError::Parse("Unclosed parenthesis(es) at end of file".into()));
        }
        if depth.bracket != 0 {
            errors.push(ParseError::Parse("Unclosed bracket(s) at end of file".into()));
        }

        errors
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

/// Track nested syntax depth.
#[derive(Default)]
struct SyntaxDepth {
    brace: i32,
    paren: i32,
    bracket: i32,
}

/// Check if a character is escaped.
fn escaped(chars: &[char], pos: usize) -> bool {
    let mut count = 0;
    let mut i = pos;
    while i > 0 {
        i -= 1;
        if chars[i] == '\\' {
            count += 1;
        } else {
            break;
        }
    }
    count % 2 == 1
}

/// Parse diagnostics module.
#[allow(dead_code)]
pub mod diagnostics {
    use crate::ParseError;

    /// Accumulated parse diagnostics.
    #[derive(Debug, Default)]
    pub struct ParseDiagnostics {
        errors: Vec<ParseError>,
    }

    impl ParseDiagnostics {
        /// Create a new diagnostics collector.
        #[must_use]
        pub const fn new() -> Self {
            Self { errors: Vec::new() }
        }

        /// Add an error.
        pub fn add_error(&mut self, err: ParseError) {
            self.errors.push(err);
        }

        /// Check if there are any errors.
        #[must_use]
        pub const fn has_errors(&self) -> bool {
            !self.errors.is_empty()
        }

        /// Get all errors.
        #[must_use]
        pub fn errors(&self) -> &[ParseError] {
            &self.errors
        }

        /// Take all errors, leaving an empty collector.
        pub fn take_errors(&mut self) -> Vec<ParseError> {
            std::mem::take(&mut self.errors)
        }
    }

    impl std::fmt::Display for ParseDiagnostics {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "Parse errors: {}", self.errors.len())
        }
    }
}
