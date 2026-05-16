//! # Type Inference Module
//!
//! Infers Rust types from TypeScript AST nodes.

mod primitives;
mod ts_types;

pub use primitives::{infer_lit, infer_bin_expr_type, infer_bin_op_result};
pub use ts_types::infer_ts_type;

use crate::analyzer::{TypeInfo, TypeMap, FunctionInfo};
use crate::analyzer::context::AnalysisContext;

/// Main type inference engine.
pub struct TypeInferrer {
    /// Inferred types for this file
    types: TypeMap,
}

impl TypeInferrer {
    /// Create a new type inferrer.
    pub fn new() -> Self {
        Self { types: TypeMap::default() }
    }

    /// Infer all types in a module.
    #[allow(unused)]
    pub fn infer_types(&mut self, _module: &(), ctx: &AnalysisContext) -> crate::Result<TypeMap> {
        // Placeholder: In full implementation, would traverse AST
        Ok(std::mem::take(&mut self.types))
    }
}

impl Default for TypeInferrer {
    fn default() -> Self {
        Self::new()
    }
}
