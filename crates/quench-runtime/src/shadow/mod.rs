//! Self-Optimizing Shadow Tree Interpreter (SSTI)
//!
//! Parallel execution path using:
//! - a `bumpalo::Bump` arena for shadow-tree nodes;
//! - NaN-boxed `JSValue`s;
//! - shape-backed objects with per-node property-read caches;
//! - an iterative, continuation-based VM that never recurses on the Rust stack.

pub mod builder;
pub mod helpers;
pub mod lower;
pub mod types;
pub mod vm;
pub mod vm_ops;

// Re-exports from submodules
pub use builder::ShadowBuilder;
pub use types::{
    AddState, Binding, Continuation, ExecType, INLINE_SLOTS, ModuleMode, ObjectLayout,
    PropCache, ShadowArena, ShadowFrame, ShadowNode, ShadowObject, TypeHint, TypeMap,
};
pub use vm::ShadowVm;
