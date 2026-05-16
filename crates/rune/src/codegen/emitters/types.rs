//! # Type Emitter
//!
//! Emits Rust type declarations from TypeScript types.

use crate::analyzer::TypeInfo;

/// Emits Rust type code.
#[derive(Debug, Default)]
pub struct TypeEmitter;

impl TypeEmitter {
    /// Create a new type emitter.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Emit a type declaration.
    #[must_use]
    pub fn emit_type(&self, type_info: &TypeInfo) -> String {
        type_info.to_rust_type()
    }
}
