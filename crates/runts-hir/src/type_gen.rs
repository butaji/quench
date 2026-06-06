//! Type code generation
//!
//! Converts HIR Type to Rust type strings
//!
//! Now delegates to type_to_rust module for shared logic.

use super::Type;
use super::type_to_rust::{TypeToRust, OutputKind};

/// Type code generator
#[allow(dead_code)]
pub struct TypeGen {
    converter: TypeToRust,
}

#[allow(dead_code)]
impl TypeGen {
    /// Create a new TypeGen
    pub fn new() -> Self {
        Self {
            converter: TypeToRust::new(OutputKind::String),
        }
    }

    /// Generate Rust type from HIR Type
    pub fn gen_type(&self, ty: &Type) -> String {
        self.converter.convert(ty).type_name()
    }
}

impl Default for TypeGen {
    fn default() -> Self {
        Self::new()
    }
}
