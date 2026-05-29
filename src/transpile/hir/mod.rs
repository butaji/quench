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

mod arena;
mod base;
mod codegen;
mod effects;
mod expr;
mod ownership;
mod pat;
mod quote_codegen;
mod stmt;
mod type_gen;

pub use arena::*;
pub use base::*;
pub use codegen::Codegen;
pub use effects::*;
pub use expr::*;
pub use ownership::*;
pub use pat::*;
pub use stmt::{ForInit, SwitchCase};
pub use type_gen::TypeGen;
pub use quote_codegen::QuoteCodegen;

/// Inference mode for type/ownership analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferenceMode {
    /// Strict mode - reject patterns that can't map to Rust
    Strict,
    /// Permissive mode - emit runtime fallbacks where needed
    Permissive,
    /// Maximum performance - optimize for known shapes
    Performance,
}
