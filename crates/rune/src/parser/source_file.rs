//! # Source File Handling
//!
//! Manages source file parsing.

use std::fs;
use std::path::Path;
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
}

impl SourceFile {
    /// Parse a source file from a path.
    pub fn parse(path: &Path, kind: SourceKind) -> Result<Self> {
        if !path.exists() {
            return Err(ParseError::NotFound(path.display().to_string()).into());
        }

        let source = fs::read_to_string(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            kind,
            source,
        })
    }

    /// Get line and column from byte offset.
    #[allow(unused)]
    pub fn location_from_offset(&self, offset: u32) -> (u32, u32) {
        let mut line = 1u32;
        let mut col = 1u32;
        let mut pos = 0u32;

        for c in self.source.chars() {
            if pos >= offset {
                break;
            }
            if c == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
            pos += 1;
        }

        (line, col)
    }

    /// Get the module body (for AST traversal placeholder).
    #[allow(unused)]
    pub fn module(&self) -> ModulePlaceholder {
        ModulePlaceholder {
            body: Vec::new(),
        }
    }
}

/// Placeholder for AST module - will be replaced with SWC integration.
#[derive(Debug, Clone)]
pub struct ModulePlaceholder {
    pub body: Vec<()>,
}

/// PathBuf type alias for clarity.
use std::path::PathBuf;
