//! Why a source did not compile: what a `SyntaxError` reports, and the offset it points at.
//! It lives with the bytecode rather than the compiler: an image without a compiler still
//! reports the diagnostics of the text it was built from.

use std::fmt;

/// The class name some compiler messages lead with.
const SYNTAX_ERROR_PREFIX: &str = "SyntaxError: ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiagnosticKind {
    Syntax,
    Compilation,
    StackExhausted,
}

#[derive(Clone)]
pub struct Diagnostic {
    pub(crate) kind: DiagnosticKind,
    pub(crate) source: String,
    pub(crate) message: String,
    /// The UTF-8 range in the source the diagnostic points at.
    pub(crate) span: (u32, u32),
}

impl Diagnostic {
    pub(crate) fn message(&self) -> &str {
        match self.kind {
            DiagnosticKind::Syntax | DiagnosticKind::Compilation => &self.message,
            DiagnosticKind::StackExhausted => crate::stack::STACK_EXHAUSTED_MESSAGE,
        }
    }

    /// Whether parsing or ECMAScript early-error validation rejected the source.
    /// Backend limits and unsupported syntax are separate compilation failures.
    pub fn is_syntax_error(&self) -> bool {
        self.kind == DiagnosticKind::Syntax
    }

    /// The UTF-8 offset in the source the diagnostic points at.
    pub fn offset(&self) -> u32 {
        self.span.0
    }

    /// The diagnostic message without a leading SyntaxError class name.
    pub fn syntax_message(&self) -> &str {
        let message = self.message();
        message.strip_prefix(SYNTAX_ERROR_PREFIX).unwrap_or(message)
    }

    pub(crate) fn is_stack_exhausted(&self) -> bool {
        self.kind == DiagnosticKind::StackExhausted
    }

    pub(crate) fn stack_exhausted(source: &str) -> Self {
        Self {
            kind: DiagnosticKind::StackExhausted,
            source: source.into(),
            message: String::new(),
            span: (0, 0),
        }
    }

    pub(crate) fn unsupported(source: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: DiagnosticKind::Compilation,
            source: source.into(),
            message: message.into(),
            span: (0, 0),
        }
    }
}

impl fmt::Debug for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Diagnostic")
            .field("source", &self.source)
            .field("message", &self.message())
            .field("span", &self.span)
            .finish()
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}..{}: {}", self.source, self.span.0, self.span.1, self.message())
    }
}
