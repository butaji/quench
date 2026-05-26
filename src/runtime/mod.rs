//! Runts runtime library
//!
//! Provides the runtime support for compiled Fresh/Preact components:
//! - Virtual DOM / Fine-grained reactivity
//! - Component system
//! - Hooks implementation
//! - Islands architecture

pub mod signals;
pub mod hooks;
pub mod component;
pub mod vdom;
pub mod islands;
pub mod prelude;

// Re-export for convenience
pub use signals::{Signal, signal, Computed};
pub use hooks::*;
pub use islands::{IslandRegistry, IslandInstance, HydrationStrategy, IslandManifest};

// HIR Interpreter for development mode
pub mod interpreter;

pub use vdom::VNode;

/// Type alias for component props
#[allow(dead_code)]
pub type Props = std::collections::HashMap<String, serde_json::Value>;

/// Type alias for component result
#[allow(dead_code)]
pub type ComponentResult = VNode;

/// Type alias for event handler (using JsValue for cross-platform compatibility)
#[allow(dead_code)]
pub type EventHandler = Box<dyn Fn(vdom::JsValue) + Send + Sync>;
