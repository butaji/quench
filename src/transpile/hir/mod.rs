//! High-level IR (Hir) for runts
//! 
//! This is an ownership-aware IR that enables direct Rust codegen.
//! 
//! Design principles:
//! - Shape specialization: known object shapes -> structs, dynamic -> HashMap
//! - Ownership inference: borrow/own/mut mirrors Rust semantics
//! - Effect inference: throw -> Result<T, E>
//! - Arena-first memory: all allocations via bumpalo arena
//! - Semantic ownership: HIR owns the semantics, not runtime strings

pub use runts_hir::*;

pub(crate) mod quote_codegen;
pub use quote_codegen::QuoteCodegen;

/// Inference mode for type/ownership analysis
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferenceMode {
    /// Strict mode - reject patterns that can't map to Rust
    Strict,
    /// Permissive mode - emit runtime fallbacks where needed
    Permissive,
    /// Maximum performance - optimize for known shapes
    Performance,
}
