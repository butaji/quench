//! Virtual DOM implementation
//!
//! This module provides the VNode type for representing elements
//! in the virtual DOM tree.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Attribute value
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttrValue {
    /// String value
    String(String),
    /// Boolean value
    Bool(bool),
    /// Number value
    Number(f64),
}

/// Virtual Node - representation of a DOM element
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum VNode {
    /// HTML/SVG element
    Element {
        /// Tag name (e.g., "div", "span", "a")
        tag: String,
        /// Attributes (key-value pairs)
        attrs: HashMap<String, AttrValue>,
        /// Event handlers (event name -> handler id)
        #[serde(default)]
        events: HashMap<String, String>,
        /// Child nodes
        #[serde(default)]
        children: Vec<VNode>,
        /// Key for reconciliation
        #[serde(skip_serializing_if = "Option::is_none")]
        key: Option<String>,
    },
    /// Text node
    Text {
        /// Text content
        value: String,
    },
    /// Fragment (multiple children without a wrapper)
    Fragment {
        /// Child nodes
        #[serde(default)]
        children: Vec<VNode>,
    },
    /// Empty node (renders nothing)
    Empty,
}

impl VNode {
    /// Create an empty node
    pub fn empty() -> Self {
        Self::Empty
    }

    /// Create a text node
    pub fn text<S: Into<String>>(value: S) -> Self {
        Self::Text { value: value.into() }
    }

    /// Create an element
    pub fn element<S: Into<String>>(tag: S) -> Self {
        Self::Element {
            tag: tag.into(),
            attrs: HashMap::new(),
            events: HashMap::new(),
            children: Vec::new(),
            key: None,
        }
    }

    /// Create a fragment
    pub fn fragment(children: Vec<VNode>) -> Self {
        Self::Fragment { children }
    }

    /// Add an attribute
    pub fn attr<S: Into<String>>(mut self, name: S, value: AttrValue) -> Self {
        if let Self::Element { attrs, .. } = &mut self {
            attrs.insert(name.into(), value);
        }
        self
    }

    /// Add a child node
    pub fn child(mut self, child: VNode) -> Self {
        if let Self::Element { children, .. } = &mut self {
            children.push(child);
        }
        self
    }

    /// Add multiple children
    pub fn children(mut self, children: Vec<VNode>) -> Self {
        if let Self::Element { children: self_children, .. } = &mut self {
            self_children.extend(children);
        }
        self
    }

    /// Set the key
    pub fn key<S: Into<String>>(mut self, key: S) -> Self {
        if let Self::Element { key: k, .. } = &mut self {
            *k = Some(key.into());
        }
        self
    }
}

impl Default for VNode {
    fn default() -> Self {
        Self::Empty
    }
}

/// Convert a VNode to HTML string
pub fn to_html(node: &VNode) -> String {
    match node {
        VNode::Empty => String::new(),
        VNode::Text { value } => escape_html(value),
        VNode::Fragment { children } => children.iter().map(to_html).collect(),
        VNode::Element { tag, attrs, children, .. } => {
            let mut html = format!("<{}", tag);
            
            // Add attributes
            for (name, value) in attrs {
                match value {
                    AttrValue::String(s) => {
                        html.push_str(&format!(" {}=\"{}\"", name, escape_attr(s)));
                    }
                    AttrValue::Bool(true) => {
                        html.push_str(&format!(" {}=\"{}\"", name, name));
                    }
                    AttrValue::Bool(false) => {
                        // Skip false booleans
                    }
                    AttrValue::Number(n) => {
                        html.push_str(&format!(" {}=\"{}\"", name, n));
                    }
                }
            }
            
            if children.is_empty() {
                // Self-closing tag
                html.push_str("/>");
            } else {
                html.push('>');
                for child in children {
                    html.push_str(&to_html(child));
                }
                html.push_str(&format!("</{}>", tag));
            }
            
            html
        }
    }
}

/// Escape HTML special characters
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Escape attribute values
fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
