//! Component system
//!
//! This module provides the component infrastructure for runts.

use super::vdom::VNode;
use std::sync::Arc;

/// Component function type
pub type ComponentFn<P> = Arc<dyn Fn(P) -> VNode + Send + Sync>;

/// A rendered component instance
pub struct ComponentInstance {
    /// The rendered VNode
    pub vnode: VNode,
    /// Unique instance ID
    pub id: String,
}

impl ComponentInstance {
    /// Create a new instance
    pub fn new(vnode: VNode) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);

        let id = COUNTER.fetch_add(1, Ordering::Relaxed);

        Self {
            vnode,
            id: format!("comp-{:x}", id),
        }
    }
}

/// Component props trait
pub trait ComponentProps: Send + Sync + 'static {
    /// Render the component
    fn render(&self) -> VNode;
}

/// Component wrapper for function components
pub struct Component<P: Send + Sync + 'static> {
    /// The component function
    pub render: Arc<dyn Fn(P) -> VNode + Send + Sync>,
}

impl<P: Send + Sync + 'static> Component<P> {
    /// Create a new component
    pub fn new<F>(render: F) -> Self
    where
        F: Fn(P) -> VNode + Send + Sync + 'static,
    {
        Self {
            render: Arc::new(render),
        }
    }

    /// Render the component with props
    pub fn render_with(&self, props: P) -> ComponentInstance {
        ComponentInstance::new((self.render)(props))
    }
}

/// Children container for passing child elements
#[derive(Debug, Clone, Default)]
pub struct Children {
    inner: Vec<VNode>,
}

impl Children {
    /// Create new empty children
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }

    /// Create from a single VNode
    pub fn from_vnode(vnode: VNode) -> Self {
        Self { inner: vec![vnode] }
    }

    /// Create from multiple VNodes
    pub fn from_vnodes(vnodes: Vec<VNode>) -> Self {
        Self { inner: vnodes }
    }

    /// Get all children
    pub fn as_slice(&self) -> &[VNode] {
        &self.inner
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Number of children
    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

impl From<VNode> for Children {
    fn from(vnode: VNode) -> Self {
        Self::from_vnode(vnode)
    }
}

impl From<Vec<VNode>> for Children {
    fn from(vnodes: Vec<VNode>) -> Self {
        Self::from_vnodes(vnodes)
    }
}

impl Extend<VNode> for Children {
    fn extend<T: IntoIterator<Item = VNode>>(&mut self, iter: T) {
        self.inner.extend(iter);
    }
}

/// Component information for registration
#[derive(Debug, Clone)]
pub struct ComponentInfo {
    /// Component name
    pub name: &'static str,
    /// Props type name (for serialization)
    pub props_type: Option<&'static str>,
}

impl ComponentInfo {
    /// Create a new component info
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            props_type: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_component_instance_new() {
        let vnode = VNode::element("div");
        let instance = ComponentInstance::new(vnode.clone());
        assert_eq!(instance.vnode, vnode);
        assert!(instance.id.starts_with("comp-"));
    }

    #[test]
    fn test_component_instance_unique_ids() {
        let inst1 = ComponentInstance::new(VNode::empty());
        let inst2 = ComponentInstance::new(VNode::empty());
        assert_ne!(inst1.id, inst2.id);
    }

    #[test]
    fn test_component_new_and_render() {
        let comp: Component<i32> = Component::new(|props: i32| {
            VNode::element("div").attr("data-value", props)
        });
        let instance = comp.render_with(42);
        assert!(matches!(instance.vnode, VNode::Element { ref tag, .. } if tag == "div"));
    }

    #[test]
    fn test_children_new() {
        let children = Children::new();
        assert!(children.is_empty());
        assert_eq!(children.len(), 0);
    }

    #[test]
    fn test_children_from_vnode() {
        let vnode = VNode::text("hello");
        let children = Children::from_vnode(vnode.clone());
        assert_eq!(children.len(), 1);
        assert_eq!(children.as_slice(), &[vnode]);
    }

    #[test]
    fn test_children_from_vnodes() {
        let vnodes = vec![VNode::text("a"), VNode::text("b"), VNode::text("c")];
        let children = Children::from_vnodes(vnodes.clone());
        assert_eq!(children.len(), 3);
        assert_eq!(children.as_slice(), vnodes.as_slice());
    }

    #[test]
    fn test_children_from_vnode_conversion() {
        let vnode = VNode::text("test");
        let children: Children = vnode.clone().into();
        assert_eq!(children.len(), 1);
        assert_eq!(children.as_slice(), &[vnode]);
    }

    #[test]
    fn test_children_from_vec_conversion() {
        let vnodes = vec![VNode::text("x"), VNode::text("y")];
        let children: Children = vnodes.clone().into();
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn test_children_extend() {
        let mut children = Children::new();
        children.extend(vec![VNode::text("a"), VNode::text("b")]);
        assert_eq!(children.len(), 2);
        children.extend(vec![VNode::text("c")]);
        assert_eq!(children.len(), 3);
    }

    #[test]
    fn test_children_is_empty() {
        let empty: Children = Children::new();
        assert!(empty.is_empty());
        let non_empty = Children::from_vnode(VNode::text("x"));
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn test_children_len() {
        let children = Children::from_vnodes(vec![
            VNode::text("a"),
            VNode::text("b"),
            VNode::text("c"),
        ]);
        assert_eq!(children.len(), 3);
    }

    #[test]
    fn test_component_info_new() {
        let info = ComponentInfo::new("Button");
        assert_eq!(info.name, "Button");
        assert_eq!(info.props_type, None);
    }

    #[test]
    fn test_component_info_clone() {
        let info1 = ComponentInfo::new("Test");
        let info2 = info1.clone();
        assert_eq!(info1.name, info2.name);
    }

    #[test]
    fn test_component_render_produces_instance() {
        let comp: Component<i32> = Component::new(|n: i32| VNode::text(n.to_string()));
        let instance = comp.render_with(42);
        assert!(instance.id.starts_with("comp-"));
    }

    #[test]
    fn test_children_clone() {
        let children1 = Children::from_vnode(VNode::text("hello"));
        let children2 = children1.clone();
        assert_eq!(children1.len(), children2.len());
    }
}
