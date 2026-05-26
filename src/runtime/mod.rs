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

pub use vdom::VNode;

/// Type alias for component props
pub type Props = std::collections::HashMap<String, serde_json::Value>;

/// Type alias for component result
pub type ComponentResult = VNode;

/// Type alias for event handler (using JsValue for cross-platform compatibility)
pub type EventHandler = Box<dyn Fn(vdom::JsValue) + Send + Sync>;
