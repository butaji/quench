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
