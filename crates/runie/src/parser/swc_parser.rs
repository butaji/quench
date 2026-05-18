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
        /// Primary error message
        message: String,
        /// Source file name
        file_name: String,
        /// Line number (1-indexed)
        line: u32,
        /// Column number (0-indexed)
        col: u32,
        /// Help/suggestion hints
        hints: Vec<String>,
    },
    /// IO error
    Io(String),
    /// Unknown error
    Unknown(String),
}

impl fmt::Display for SwcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SwcError::Parse {
                message,
                file_name,
                line,
                col,
                hints,
            } => {
                write!(f, "{}:{}:{}: {}", file_name, line, col, message)?;
                if !hints.is_empty() {
                    write!(f, " ({})", hints.join("; "))?;
                }
                Ok(())
            }
            SwcError::Io(msg) => write!(f, "IO error: {}", msg),
            SwcError::Unknown(msg) => write!(f, "Unknown error: {}", msg),
        }
    }
}

impl std::error::Error for SwcError {}

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

            // Extract the error message from debug output
            let err_str = format!("{:?}", e);
            let message = Self::extract_error_message(&err_str);
            let hints = Self::generate_hints(&err_str);

            e.into_diagnostic(&handler).emit();

            SwcError::Parse {
                message,
                file_name: file_name.to_string(),
                line: loc.line as u32,
                col: loc.col.0 as u32,
                hints,
            }
        })?;

        Ok(Self {
            module,
            source_map: cm,
            file_name: file_name.to_string(),
            source_text: source.to_string(),
        })
    }

    /// Extract a human-readable error message from the debug string.
    fn extract_error_message(err_str: &str) -> String {
        err_str
            .lines()
            .next()
            .map(|l| l.trim().to_string())
            .unwrap_or_else(|| "Parse error".to_string())
    }

    /// Generate helpful hints based on the error message.
    fn generate_hints(err_str: &str) -> Vec<String> {
        let msg_lower = err_str.to_lowercase();
        let mut hints = Vec::new();

        if msg_lower.contains("unexpected end of input") {
            hints.push("missing closing brace or parenthesis".to_string());
        }
        if msg_lower.contains("expected") && msg_lower.contains("but got") {
            hints.push("check brackets and operators".to_string());
        }
        if msg_lower.contains("semicolon") {
            hints.push("add semicolon".to_string());
        }
        if msg_lower.contains("unterminated") {
            hints.push("check string or template literal".to_string());
        }

        hints
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
