//! Ink runtime state management
//!
//! Manages the node tree and coordinates layout calculations.

use crate::ink::{InkNode, InkTag, PropValue};
use ordered_float::OrderedFloat;
use std::collections::HashMap;
use thiserror::Error;
use yoga::StyleUnit;

pub type Result<T> = std::result::Result<T, InkError>;

/// Ink-specific errors
#[derive(Error, Debug)]
pub enum InkError {
    #[error("Node {0} not found")]
    NodeNotFound(u32),

    #[error("Invalid node type: {0}")]
    InvalidNodeType(String),

    #[error("Layout error: {0}")]
    LayoutError(String),

    #[error("Render error: {0}")]
    RenderError(String),

    #[error("Node {0} not found during insert_before")]
    InsertBeforeError(u32),
}

/// The Ink runtime state
pub struct InkRuntime {
    pub(crate) next_id: u32,
    pub(crate) nodes: Vec<Option<InkNode>>,
    pub(crate) root_id: Option<u32>,
    pub(crate) dirty: bool,
    pub(crate) terminal_width: f32,
    pub(crate) terminal_height: f32,
}

impl InkRuntime {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            nodes: Vec::new(),
            root_id: None,
            dirty: false,
            terminal_width: 80.0,
            terminal_height: 24.0,
        }
    }

    /// Create the root node
    pub fn create_root(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;

        while self.nodes.len() <= id as usize {
            self.nodes.push(None);
        }

        let mut root = InkNode::new(id, InkTag::Box);
        root.yoga.set_width(StyleUnit::Percent(OrderedFloat(100.0)));
        root.yoga.set_height(StyleUnit::Percent(OrderedFloat(100.0)));
        root.yoga.set_flex_direction(yoga::FlexDirection::Column);

        self.root_id = Some(id);
        self.nodes[id as usize] = Some(root);
        id
    }

    /// Destroy the root and all nodes
    pub fn destroy_root(&mut self, root_id: u32) {
        if self.root_id == Some(root_id) {
            for node_opt in self.nodes.iter_mut() {
                *node_opt = None;
            }
            self.nodes.clear();
            self.root_id = None;
        }
    }

    /// Create a new node
    pub fn create_node(&mut self, tag: &str, props: HashMap<String, PropValue>) -> u32 {
        let id = self.next_id;
        self.next_id += 1;

        while self.nodes.len() <= id as usize {
            self.nodes.push(None);
        }

        let tag = InkTag::from_str(tag);
        let mut node = InkNode::new(id, tag);
        node.apply_props(&props);

        self.nodes[id as usize] = Some(node);
        self.dirty = true;
        id
    }

    /// Create a text node
    pub fn create_text_node(&mut self, text: &str) -> u32 {
        let id = self.next_id;
        self.next_id += 1;

        while self.nodes.len() <= id as usize {
            self.nodes.push(None);
        }

        let node = InkNode::new_text(id, text.to_string());
        self.nodes[id as usize] = Some(node);
        self.dirty = true;
        id
    }

    pub(crate) fn get_node(&self, id: u32) -> Option<&InkNode> {
        self.nodes.get(id as usize).and_then(|n| n.as_ref())
    }

    pub(crate) fn get_node_mut(&mut self, id: u32) -> Option<&mut InkNode> {
        self.nodes.get_mut(id as usize).and_then(|n| n.as_mut())
    }

    /// Mark dirty
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Check if dirty
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Clear dirty flag
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// Set terminal dimensions
    pub fn set_terminal_size(&mut self, width: u32, height: u32) {
        self.terminal_width = width as f32;
        self.terminal_height = height as f32;
        self.dirty = true;
    }

    /// Get terminal dimensions
    pub fn terminal_size(&self) -> (u32, u32) {
        (self.terminal_width as u32, self.terminal_height as u32)
    }

    /// Calculate layout for the tree
    pub fn calculate_layout(&mut self) -> Result<()> {
        let Some(root_id) = self.root_id else {
            return Ok(());
        };

        let (width, height) = (self.terminal_width, self.terminal_height);

        let root = self.get_node_mut(root_id).ok_or(InkError::NodeNotFound(root_id))?;

        root.yoga.set_width(StyleUnit::Point(OrderedFloat(width)));
        root.yoga.set_height(StyleUnit::Point(OrderedFloat(height)));

        root.yoga.calculate_layout(width, height, yoga::Direction::LTR);

        Ok(())
    }

    /// Get the root node ID
    pub fn root_id(&self) -> Option<u32> {
        self.root_id
    }

    /// Get a node by ID
    pub fn node(&self, id: u32) -> Option<&InkNode> {
        self.get_node(id)
    }

    /// Get a mutable node by ID
    pub fn node_mut(&mut self, id: u32) -> Option<&mut InkNode> {
        self.get_node_mut(id)
    }
}

impl Default for InkRuntime {
    fn default() -> Self {
        Self::new()
    }
}
