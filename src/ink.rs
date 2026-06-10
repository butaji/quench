//! Ink runtime management — Yoga tree, rendering, and reconciler state
//!
//! This module manages the node tree, Yoga layout calculations, and coordinates
//! with the rendering layer.

use std::collections::HashMap;

use ordered_float::OrderedFloat;
use yoga::{Node, Layout, Align, Display, FlexDirection, Justify, PositionType, Wrap, StyleUnit};
use thiserror::Error;

/// Tag types matching React component names from Ink
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InkTag {
    Box,
    Text,
    Static,
    Newline,
    Spacer,
    Unknown,
}

impl InkTag {
    pub fn from_str(s: &str) -> Self {
        match s {
            "ink-box" => InkTag::Box,
            "ink-text" => InkTag::Text,
            "ink-static" => InkTag::Static,
            "ink-newline" => InkTag::Newline,
            "ink-spacer" => InkTag::Spacer,
            _ => InkTag::Unknown,
        }
    }
}

/// A single Ink node in the tree
pub struct InkNode {
    /// Unique identifier
    pub id: u32,
    /// Node type
    pub tag: InkTag,
    /// Flex properties from JSX props
    pub props: HashMap<String, PropValue>,
    /// Text content (for text nodes)
    pub text: Option<String>,
    /// Parent node ID (None for root)
    pub parent: Option<u32>,
    /// Child node IDs
    pub children: Vec<u32>,
    /// Yoga layout node (wrapped in RefCell for interior mutability)
    pub yoga: Node,
}

impl InkNode {
    pub fn new(id: u32, tag: InkTag) -> Self {
        let mut yoga = Node::new();
        // Default flex settings for terminal layout
        yoga.set_flex_direction(FlexDirection::Row);
        yoga.set_align_items(Align::FlexStart);
        yoga.set_justify_content(Justify::FlexStart);
        
        Self {
            id,
            tag,
            props: HashMap::new(),
            text: None,
            parent: None,
            children: Vec::new(),
            yoga,
        }
    }

    pub fn new_text(id: u32, text: String) -> Self {
        let mut node = Self::new(id, InkTag::Text);
        node.text = Some(text);
        node
    }

    pub fn new_spacer(id: u32) -> Self {
        let mut node = Self::new(id, InkTag::Spacer);
        // Spacer takes up remaining space
        node.yoga.set_flex_grow(1.0);
        node.yoga.set_flex_shrink(1.0);
        node
    }

    pub fn new_newline(id: u32) -> Self {
        let mut node = Self::new(id, InkTag::Newline);
        node.yoga.set_flex_direction(FlexDirection::Column);
        node
    }

    /// Apply props to the Yoga node
    pub fn apply_props(&mut self, props: &HashMap<String, PropValue>) {
        self.props = props.clone();
        
        // Map Ink props to Yoga properties
        if let Some(PropValue::String(s)) = props.get("flexDirection") {
            self.yoga.set_flex_direction(match s.as_str() {
                "column" => FlexDirection::Column,
                "column-reverse" => FlexDirection::ColumnReverse,
                "row-reverse" => FlexDirection::RowReverse,
                _ => FlexDirection::Row,
            });
        }

        if let Some(PropValue::String(s)) = props.get("alignItems") {
            self.yoga.set_align_items(match s.as_str() {
                "center" => Align::Center,
                "flex-end" => Align::FlexEnd,
                "stretch" => Align::Stretch,
                "baseline" => Align::Baseline,
                _ => Align::FlexStart,
            });
        }

        if let Some(PropValue::String(s)) = props.get("justifyContent") {
            self.yoga.set_justify_content(match s.as_str() {
                "center" => Justify::Center,
                "flex-end" => Justify::FlexEnd,
                "space-between" => Justify::SpaceBetween,
                "space-around" => Justify::SpaceAround,
                "space-evenly" => Justify::SpaceEvenly,
                _ => Justify::FlexStart,
            });
        }

        if let Some(PropValue::String(s)) = props.get("flexWrap") {
            self.yoga.set_flex_wrap(match s.as_str() {
                "wrap" => Wrap::Wrap,
                "nowrap" => Wrap::NoWrap,
                _ => Wrap::NoWrap,
            });
        }

        if let Some(PropValue::String(s)) = props.get("display") {
            self.yoga.set_display(match s.as_str() {
                "flex" => Display::Flex,
                "none" => Display::None,
                _ => Display::Flex,
            });
        }

        // Spacing: margin, padding
        if let Some(v) = props.get("margin").and_then(parse_spacing_value) {
            self.yoga.set_margin(yoga::Edge::All, StyleUnit::Point(OrderedFloat(v)));
        }

        // marginTop, marginBottom, marginLeft, marginRight
        if let Some(v) = props.get("marginTop").and_then(parse_spacing_value) {
            self.yoga.set_margin(yoga::Edge::Top, StyleUnit::Point(OrderedFloat(v)));
        }
        if let Some(v) = props.get("marginBottom").and_then(parse_spacing_value) {
            self.yoga.set_margin(yoga::Edge::Bottom, StyleUnit::Point(OrderedFloat(v)));
        }
        if let Some(v) = props.get("marginLeft").and_then(parse_spacing_value) {
            self.yoga.set_margin(yoga::Edge::Left, StyleUnit::Point(OrderedFloat(v)));
        }
        if let Some(v) = props.get("marginRight").and_then(parse_spacing_value) {
            self.yoga.set_margin(yoga::Edge::Right, StyleUnit::Point(OrderedFloat(v)));
        }
        // marginY = top + bottom, marginX = left + right
        if let Some(v) = props.get("marginY").and_then(parse_spacing_value) {
            self.yoga.set_margin(yoga::Edge::Top, StyleUnit::Point(OrderedFloat(v)));
            self.yoga.set_margin(yoga::Edge::Bottom, StyleUnit::Point(OrderedFloat(v)));
        }
        if let Some(v) = props.get("marginX").and_then(parse_spacing_value) {
            self.yoga.set_margin(yoga::Edge::Left, StyleUnit::Point(OrderedFloat(v)));
            self.yoga.set_margin(yoga::Edge::Right, StyleUnit::Point(OrderedFloat(v)));
        }

        // paddingTop, paddingBottom, paddingLeft, paddingRight
        if let Some(v) = props.get("paddingTop").and_then(parse_spacing_value) {
            self.yoga.set_padding(yoga::Edge::Top, StyleUnit::Point(OrderedFloat(v)));
        }
        if let Some(v) = props.get("paddingBottom").and_then(parse_spacing_value) {
            self.yoga.set_padding(yoga::Edge::Bottom, StyleUnit::Point(OrderedFloat(v)));
        }
        if let Some(v) = props.get("paddingLeft").and_then(parse_spacing_value) {
            self.yoga.set_padding(yoga::Edge::Left, StyleUnit::Point(OrderedFloat(v)));
        }
        if let Some(v) = props.get("paddingRight").and_then(parse_spacing_value) {
            self.yoga.set_padding(yoga::Edge::Right, StyleUnit::Point(OrderedFloat(v)));
        }
        if let Some(v) = props.get("padding").and_then(parse_spacing_value) {
            self.yoga.set_padding(yoga::Edge::All, StyleUnit::Point(OrderedFloat(v)));
        }
        // paddingY = top + bottom, paddingX = left + right
        if let Some(v) = props.get("paddingY").and_then(parse_spacing_value) {
            self.yoga.set_padding(yoga::Edge::Top, StyleUnit::Point(OrderedFloat(v)));
            self.yoga.set_padding(yoga::Edge::Bottom, StyleUnit::Point(OrderedFloat(v)));
        }
        if let Some(v) = props.get("paddingX").and_then(parse_spacing_value) {
            self.yoga.set_padding(yoga::Edge::Left, StyleUnit::Point(OrderedFloat(v)));
            self.yoga.set_padding(yoga::Edge::Right, StyleUnit::Point(OrderedFloat(v)));
        }

        // Borders
        if let Some(PropValue::String(s)) = props.get("borderStyle") {
            let border_width = match s.as_str() {
                "single" | "bold" | "round" | "double" | "singleDouble" 
                | "doubleSingle" | "Classic" | "Pascal" | "嘴里" => 1.0,
                "none" => 0.0,
                _ => 0.0,
            };
            if border_width > 0.0 {
                self.yoga.set_border(yoga::Edge::Left, border_width);
                self.yoga.set_border(yoga::Edge::Top, border_width);
                self.yoga.set_border(yoga::Edge::Right, border_width);
                self.yoga.set_border(yoga::Edge::Bottom, border_width);
            }
        }

        // Width/height
        if let Some(v) = props.get("width") {
            match v {
                PropValue::Number(n) => {
                    self.yoga.set_width(StyleUnit::Point(OrderedFloat(*n as f32)));
                }
                PropValue::String(s) if s == "auto" => {
                    self.yoga.set_width(StyleUnit::Auto);
                }
                PropValue::String(s) if s.ends_with('%') => {
                    if let Ok(pct) = s.trim_end_matches('%').parse::<f32>() {
                        self.yoga.set_width(StyleUnit::Percent(OrderedFloat(pct)));
                    }
                }
                _ => {}
            }
        }

        if let Some(v) = props.get("height") {
            match v {
                PropValue::Number(n) => {
                    self.yoga.set_height(StyleUnit::Point(OrderedFloat(*n as f32)));
                }
                PropValue::String(s) if s == "auto" => {
                    self.yoga.set_height(StyleUnit::Auto);
                }
                PropValue::String(s) if s.ends_with('%') => {
                    if let Ok(pct) = s.trim_end_matches('%').parse::<f32>() {
                        self.yoga.set_height(StyleUnit::Percent(OrderedFloat(pct)));
                    }
                }
                _ => {}
            }
        }

        // Position
        if let Some(PropValue::String(s)) = props.get("position") {
            self.yoga.set_position_type(match s.as_str() {
                "absolute" => PositionType::Absolute,
                _ => PositionType::Relative,
            });
        }

        // Flex grow (not on Spacer which hardcodes it)
        if self.tag != InkTag::Spacer {
            if let Some(PropValue::Number(n)) = props.get("flexGrow") {
                self.yoga.set_flex_grow(*n as f32);
            }
            if let Some(PropValue::Number(n)) = props.get("flexShrink") {
                self.yoga.set_flex_shrink(*n as f32);
            }
        }

        // Flex basis
        if let Some(v) = props.get("flexBasis") {
            match v {
                PropValue::Number(n) => {
                    self.yoga.set_flex_basis(StyleUnit::Point(OrderedFloat(*n as f32)));
                }
                PropValue::String(s) if s == "auto" => {
                    self.yoga.set_flex_basis(StyleUnit::Auto);
                }
                PropValue::String(s) if s.ends_with('%') => {
                    if let Ok(pct) = s.trim_end_matches('%').parse::<f32>() {
                        self.yoga.set_flex_basis(StyleUnit::Percent(OrderedFloat(pct)));
                    }
                }
                _ => {}
            }
        }

        // Min/max dimensions
        if let Some(v) = props.get("minWidth") {
            match v {
                PropValue::Number(n) => {
                    self.yoga.set_min_width(StyleUnit::Point(OrderedFloat(*n as f32)));
                }
                PropValue::String(s) if s.ends_with('%') => {
                    if let Ok(pct) = s.trim_end_matches('%').parse::<f32>() {
                        self.yoga.set_min_width(StyleUnit::Percent(OrderedFloat(pct)));
                    }
                }
                _ => {}
            }
        }
        if let Some(v) = props.get("maxWidth") {
            match v {
                PropValue::Number(n) => {
                    self.yoga.set_max_width(StyleUnit::Point(OrderedFloat(*n as f32)));
                }
                PropValue::String(s) if s.ends_with('%') => {
                    if let Ok(pct) = s.trim_end_matches('%').parse::<f32>() {
                        self.yoga.set_max_width(StyleUnit::Percent(OrderedFloat(pct)));
                    }
                }
                _ => {}
            }
        }
        if let Some(v) = props.get("minHeight") {
            match v {
                PropValue::Number(n) => {
                    self.yoga.set_min_height(StyleUnit::Point(OrderedFloat(*n as f32)));
                }
                PropValue::String(s) if s.ends_with('%') => {
                    if let Ok(pct) = s.trim_end_matches('%').parse::<f32>() {
                        self.yoga.set_min_height(StyleUnit::Percent(OrderedFloat(pct)));
                    }
                }
                _ => {}
            }
        }
        if let Some(v) = props.get("maxHeight") {
            match v {
                PropValue::Number(n) => {
                    self.yoga.set_max_height(StyleUnit::Point(OrderedFloat(*n as f32)));
                }
                PropValue::String(s) if s.ends_with('%') => {
                    if let Ok(pct) = s.trim_end_matches('%').parse::<f32>() {
                        self.yoga.set_max_height(StyleUnit::Percent(OrderedFloat(pct)));
                    }
                }
                _ => {}
            }
        }
    }

    /// Get computed layout
    pub fn get_layout(&self) -> Layout {
        self.yoga.get_layout()
    }
}

/// Parse spacing string to f32 value
fn parse_spacing_value(v: &PropValue) -> Option<f32> {
    match v {
        PropValue::Number(n) => Some(*n as f32),
        PropValue::String(s) => {
            let s = s.trim_end_matches("px").trim();
            s.parse().ok()
        }
        _ => None,
    }
}

/// Property value types from JSX props
#[derive(Debug, Clone, PartialEq)]
pub enum PropValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Vec(Vec<PropValue>),
}

/// The Ink runtime state - uses Rc<RefCell<...>> for interior mutability
/// since Yoga Node is not Send+Sync
pub struct InkRuntime {
    /// Next available node ID
    next_id: u32,
    /// All nodes by ID (uses indices for Yoga node access)
    nodes: Vec<Option<InkNode>>,
    /// Root node ID
    root_id: Option<u32>,
    /// Whether a commit is pending
    dirty: bool,
    /// Terminal dimensions (updated by event loop)
    terminal_width: f32,
    terminal_height: f32,
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

        // Ensure vector is large enough
        while self.nodes.len() <= id as usize {
            self.nodes.push(None);
        }

        let mut root = InkNode::new(id, InkTag::Box);
        // Root fills the terminal
        root.yoga.set_width(StyleUnit::Percent(OrderedFloat(100.0)));
        root.yoga.set_height(StyleUnit::Percent(OrderedFloat(100.0)));
        root.yoga.set_flex_direction(FlexDirection::Column);

        self.root_id = Some(id);
        self.nodes[id as usize] = Some(root);
        id
    }

    /// Destroy the root and all nodes
    pub fn destroy_root(&mut self, root_id: u32) {
        if self.root_id == Some(root_id) {
            // Clear all nodes
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

        // Ensure vector is large enough
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

        // Ensure vector is large enough
        while self.nodes.len() <= id as usize {
            self.nodes.push(None);
        }

        let node = InkNode::new_text(id, text.to_string());
        self.nodes[id as usize] = Some(node);
        self.dirty = true;
        id
    }

    /// Get a node by ID
    fn get_node(&self, id: u32) -> Option<&InkNode> {
        self.nodes.get(id as usize).and_then(|n| n.as_ref())
    }

    /// Get a mutable node by ID
    fn get_node_mut(&mut self, id: u32) -> Option<&mut InkNode> {
        self.nodes.get_mut(id as usize).and_then(|n| n.as_mut())
    }

    /// Append child to parent
    pub fn append_child(&mut self, parent_id: u32, child_id: u32) -> Result<()> {
        // First get child's info while we can borrow immutably
        let (child_yoga_ptr, old_parent_id) = {
            let child = self.get_node(child_id)
                .ok_or(InkError::NodeNotFound(child_id))?;
            let ptr = &child.yoga as *const Node as *mut Node;
            (ptr, child.parent)
        };
        
        // Remove from old parent's Yoga tree if moving from a different parent
        if let Some(old_pid) = old_parent_id {
            if old_pid != parent_id {
                if let Some(old_parent) = self.get_node_mut(old_pid) {
                    old_parent.children.retain(|&id| id != child_id);
                    unsafe {
                        old_parent.yoga.remove_child(&mut *child_yoga_ptr);
                    }
                }
            }
        }
        
        // Get parent mutably
        let parent = self.get_node_mut(parent_id)
            .ok_or(InkError::NodeNotFound(parent_id))?;

        // Insert into parent's children list
        parent.children.push(child_id);
        
        // Insert into Yoga tree
        // Safety: child_yoga_ptr points to a valid Node in self.nodes
        unsafe {
            parent.yoga.insert_child(&mut *child_yoga_ptr, parent.children.len() - 1);
        }
        
        // Update child's parent
        if let Some(child_node) = self.get_node_mut(child_id) {
            child_node.parent = Some(parent_id);
        }

        self.dirty = true;
        Ok(())
    }

    /// Remove child from parent
    pub fn remove_child(&mut self, parent_id: u32, child_id: u32) -> Result<()> {
        let child_ptr = self.get_node(child_id)
            .map(|c| &c.yoga as *const Node as *mut Node);

        let parent = self.get_node_mut(parent_id)
            .ok_or(InkError::NodeNotFound(parent_id))?;

        if let Some(ptr) = child_ptr {
            unsafe {
                parent.yoga.remove_child(&mut *ptr);
            }
        }

        parent.children.retain(|&id| id != child_id);

        if let Some(child) = self.get_node_mut(child_id) {
            child.parent = None;
        }

        self.dirty = true;
        Ok(())
    }

    /// Insert child before another child
    pub fn insert_before(&mut self, parent_id: u32, child_id: u32, before_id: u32) -> Result<()> {
        // Collect ALL info we need while borrowing immutably - this is critical
        let (insert_idx, old_parent_id, child_ptrs) = {
            let parent = self.get_node(parent_id)
                .ok_or(InkError::NodeNotFound(parent_id))?;
            let insert_idx = parent.children.iter().position(|&id| id == before_id)
                .ok_or(InkError::InsertBeforeError(before_id))?;
            let old_parent_id = self.get_node(child_id)
                .and_then(|c| c.parent);
            // Collect ALL Yoga pointers for ALL nodes we might touch
            let child_ptrs: Vec<(*mut Node, u32)> = parent.children.iter()
                .filter(|&&id| id != child_id)
                .filter_map(|&id| {
                    self.get_node(id).map(|n| {
                        (&n.yoga as *const Node as *mut Node, id)
                    })
                })
                .collect();
            (insert_idx, old_parent_id, child_ptrs)
        };

        // Now we can get mutable access to parent
        let parent = self.get_node_mut(parent_id)
            .ok_or(InkError::NodeNotFound(parent_id))?;

        // Track old parent for later
        let old_pid_for_later = if old_parent_id != Some(parent_id) { old_parent_id } else { None };

        // Remove from parent.children if already present
        parent.children.retain(|&id| id != child_id);
        
        // Insert at new position
        parent.children.insert(insert_idx, child_id);

        // Rebuild Yoga children using the collected pointers
        // We use the pointers that were collected before any mutation
        for (i, &(ptr, _)) in child_ptrs.iter().enumerate() {
            unsafe {
                parent.yoga.insert_child(&mut *ptr, i);
            }
        }

        // Drop parent borrow so we can access old parent
        let _ = parent;
        
        // Remove from old parent if different
        if let Some(old_pid) = old_pid_for_later {
            if let Some(old_parent) = self.get_node_mut(old_pid) {
                old_parent.children.retain(|&id| id != child_id);
            }
        }

        if let Some(child) = self.get_node_mut(child_id) {
            child.parent = Some(parent_id);
        }

        self.dirty = true;
        Ok(())
    }

    /// Commit an update to a node's props
    pub fn commit_update(&mut self, node_id: u32, props: HashMap<String, PropValue>) -> Result<()> {
        let node = self.get_node_mut(node_id)
            .ok_or(InkError::NodeNotFound(node_id))?;
        node.apply_props(&props);
        self.dirty = true;
        Ok(())
    }

    /// Set text content of a text node
    pub fn set_text(&mut self, node_id: u32, text: &str) -> Result<()> {
        let node = self.get_node_mut(node_id)
            .ok_or(InkError::NodeNotFound(node_id))?;
        node.text = Some(text.to_string());
        self.dirty = true;
        Ok(())
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
        
        let root = self.get_node_mut(root_id)
            .ok_or(InkError::NodeNotFound(root_id))?;

        root.yoga.set_width(StyleUnit::Point(OrderedFloat(width)));
        root.yoga.set_height(StyleUnit::Point(OrderedFloat(height)));
        
        root.yoga.calculate_layout(width, height, yoga::Direction::LTR);

        Ok(())
    }

    /// Get the root node ID
    pub fn root_id(&self) -> Option<u32> {
        self.root_id
    }

    /// Get a node by ID (public)
    pub fn node(&self, id: u32) -> Option<&InkNode> {
        self.get_node(id)
    }

    /// Get a mutable node by ID (public)
    pub fn node_mut(&mut self, id: u32) -> Option<&mut InkNode> {
        self.get_node_mut(id)
    }
}

impl Default for InkRuntime {
    fn default() -> Self {
        Self::new()
    }
}

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

pub type Result<T> = std::result::Result<T, InkError>;
