//! Runts runtime prelude
//!
//! This module provides convenient access to all commonly used runtime types.
//! Import with `use runts_lib::runtime::prelude::*;`

// Re-export commonly used types
pub use super::vdom::VNode;

// Re-export for component macro

// Note: For html! macro with full JSX support, use runts_macros::html!
// The basic VNode construction is available via VNode::element() builder

// Common type aliases
#[allow(dead_code)]
pub type Props = std::collections::HashMap<String, serde_json::Value>;
#[allow(dead_code)]
pub type Children = Vec<VNode>;

/// Helper for rendering multiple children
#[macro_export]
macro_rules! children {
    () => {
        vec![]
    };
    ($($item:expr),*) => {
        vec![$($item.into_vnode()),*]
    };
}
