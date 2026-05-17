//! # SWC Parser Integration
//!
//! Parses TypeScript using SWC into an AST representation.

use std::fmt;
use std::io::sink;
use swc_common::{errors::Handler, sync::Lrc, FileName, SourceMap, Spanned};
use swc_ecma_ast::{Module, ModuleItem};
use swc_ecma_parser::{Parser, StringInput, Syntax, TsSyntax};

/// Result type for SWC parsing.
pub type SwcResult<T> = Result<T, SwcError>;

/// Errors from SWC parsing.
#[derive(Debug, Clone)]
pub enum SwcError {
    /// Parse error with location
    Parse {
        message: String,
        line: u32,
        col: u32,
    },
    /// IO error
    Io(String),
    /// Unknown error
    Unknown(String),
}

impl fmt::Display for SwcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SwcError::Parse { message, line, col } => {
                write!(f, "Parse error at {}:{}: {}", line, col, message)
            }
            SwcError::Io(msg) => write!(f, "IO error: {}", msg),
            SwcError::Unknown(msg) => write!(f, "Unknown error: {}", msg),
        }
    }
}

/// SWC AST wrapper for easier traversal.
#[derive(Clone)]
pub struct SwcAst {
    /// Module AST from SWC
    pub module: Module,
    /// Source map
    pub source_map: Lrc<SourceMap>,
    /// Source file name
    pub file_name: String,
    /// Original source text
    pub source_text: String,
}

impl SwcAst {
    /// Parse TypeScript source with SWC.
    pub fn parse_ts(source: &str, file_name: &str) -> SwcResult<Self> {
        Self::parse_with_swc(source, file_name, false)
    }

    /// Parse TSX source with SWC.
    pub fn parse_tsx(source: &str, file_name: &str) -> SwcResult<Self> {
        Self::parse_with_swc(source, file_name, true)
    }

    fn parse_with_swc(source: &str, file_name: &str, tsx: bool) -> SwcResult<Self> {
        let cm = Lrc::new(SourceMap::default());
        let fm = cm.new_source_file(
            FileName::Custom(file_name.to_string()).into(),
            source.to_string(),
        );

        let handler = Handler::with_emitter_writer(Box::new(sink()), Some(cm.clone()));

        let mut lexer = Parser::new(
            Syntax::Typescript(TsSyntax {
                tsx,
                ..Default::default()
            }),
            StringInput::from(&*fm),
            None,
        );

        let module = lexer.parse_module().map_err(|e| {
            let span = e.span();
            let loc = cm.lookup_char_pos(span.lo);
            let msg = format!("Parse error at {}:{}: {:?}", loc.line, loc.col.0, e);
            e.into_diagnostic(&handler).emit();
            SwcError::Parse {
                message: msg,
                line: loc.line as u32,
                col: loc.col.0 as u32,
            }
        })?;

        Ok(Self {
            module,
            source_map: cm,
            file_name: file_name.to_string(),
            source_text: source.to_string(),
        })
    }

    /// Get line and column from byte offset.
    pub fn location_from_offset(&self, offset: u32) -> (u32, u32) {
        use swc_common::BytePos;
        let loc = self.source_map.lookup_char_pos(BytePos(offset));
        (loc.line as u32, loc.col.0 as u32)
    }

    /// Get all top-level statements.
    pub fn statements(&self) -> &[ModuleItem] {
        &self.module.body
    }
}
