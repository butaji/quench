//! High-level IR (Hir) for runts
//! 
//! This is an ownership-aware IR that enables direct Rust codegen.
//! 
//! Design principles:
//! - Shape specialization: known object shapes -> structs, dynamic -> HashMap
//! - Ownership inference: borrow/own/mut mirrors Rust semantics
//! - Arena-first memory: all allocations via bumpalo arena
//! - Semantic ownership: HIR owns the semantics, not runtime strings

mod base;
mod codegen;
mod expr;
mod ownership;
mod pat;
mod stmt;

pub use base::*;
pub use codegen::Codegen;
pub use expr::*;
pub use ownership::*;
pub use pat::*;
pub use stmt::{ForInit, SwitchCase};

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
