//! # Source File Handling
//!
//! Manages source file parsing with SWC and provides the AST.

use std::fs;
use std::path::Path;
use swc_common::{
    errors::{ColorConfig, Handler},
    sync::Lrc,
    FileName, SourceMap,
};
use swc_ecma_ast::*;
use swc_ecma_parser::{lexer::Lexer, Parser, StringInput, Syntax, TsConfig};

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
    /// SWC AST module
    pub module: Module,
    /// Source map reference
    pub source_map: Lrc<SourceMap>,
}

impl SourceFile {
    /// Parse a source file from a path.
    pub fn parse(path: &Path, kind: SourceKind) -> Result<Self> {
        if !path.exists() {
            return Err(ParseError::NotFound(path.display().to_string()).into());
        }

        let source = fs::read_to_string(path)?;
        let path_str = path.display().to_string();

        let cm: Lrc<SourceMap> = Default::default();
        let fm = cm.new_source_file(FileName::Real(path_str.clone().into()).into(), source.clone());

        let syntax = match kind {
            SourceKind::TypeScript => Syntax::Typescript(TsConfig {
                tsx: false,
                decorators: false,
                dts: false,
                no_early_errors: false,
                disallow_ambiguous_jsx_like: true,
            }),
            SourceKind::Tsx => Syntax::Typescript(TsConfig {
                tsx: true,
                decorators: false,
                dts: false,
                no_early_errors: false,
                disallow_ambiguous_jsx_like: true,
            }),
        };

        let lexer = Lexer::new(
            syntax,
            Default::default(),
            StringInput::from(&*fm),
            None,
        );

        let mut parser = Parser::new_from(lexer);
        let module = parser
            .parse_module()
            .map_err(|e| ParseError::Swc(e.to_string()))?;

        Ok(Self {
            path: path.to_path_buf(),
            kind,
            source,
            module,
            source_map: cm,
        })
    }

    /// Parse a Rust file (for native interop).
    pub fn from_rust_file(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(ParseError::NotFound(path.display().to_string()).into());
        }

        let source = fs::read_to_string(path)?;
        let path_str = path.display().to_string();

        let cm: Lrc<SourceMap> = Default::default();
        let fm = cm.new_source_file(FileName::Real(path_str.clone().into()).into(), source.clone());

        let lexer = Lexer::new(
            Syntax::Typescript(TsConfig {
                tsx: false,
                decorators: false,
                dts: false,
                no_early_errors: false,
                disallow_ambiguous_jsx_like: true,
            }),
            Default::default(),
            StringInput::from(&*fm),
            None,
        );

        let mut parser = Parser::new_from(lexer);
        let module = parser
            .parse_module()
            .map_err(|e| ParseError::Swc(e.to_string()))?;

        Ok(Self {
            path: path.to_path_buf(),
            kind: SourceKind::TypeScript,
            source,
            module,
            source_map: cm,
        })
    }

    /// Get line and column from byte offset.
    pub fn location_from_offset(&self, offset: u32) -> (u32, u32) {
        let fm = self.source_map.get_source_file(&self.module.span).unwrap();
        let loc = fm.lookup_char_pos(swc_common::BytePos(offset));
        (loc.line, loc.col.0 + 1)
    }
}

/// PathBuf type alias for clarity.
use std::path::PathBuf;
