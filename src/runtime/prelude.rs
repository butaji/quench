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
pub type Props = std::collections::HashMap<String, serde_json::Value>;
pub type Children = Vec<VNode>;

/// Trait for types that can be used as children in components
pub trait IntoVNode {
    fn into_vnode(self) -> VNode;
}

impl IntoVNode for VNode {
    fn into_vnode(self) -> VNode {
        self
    }
}

impl IntoVNode for String {
    fn into_vnode(self) -> VNode {
        VNode::Text { value: self }
    }
}

impl IntoVNode for &str {
    fn into_vnode(self) -> VNode {
        VNode::Text { value: self.to_string() }
    }
}

impl IntoVNode for () {
    fn into_vnode(self) -> VNode {
        VNode::Empty
    }
}

impl<T: IntoVNode> IntoVNode for Option<T> {
    fn into_vnode(self) -> VNode {
        match self {
            Some(v) => v.into_vnode(),
            None => VNode::Empty,
        }
    }
}

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
