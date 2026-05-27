//! Virtual DOM implementation
//!
//! This module provides the VNode type for representing elements
//! in the virtual DOM tree.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Attribute value
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
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
    /// Component placeholder (rendered by the component function)
    Component {
        /// Component name
        name: String,
        /// Props
        #[serde(default)]
        props: std::collections::HashMap<String, serde_json::Value>,
        /// Children
        #[serde(default)]
        children: Vec<VNode>,
    },
    /// Fragment (multiple children without a wrapper)
    Fragment {
        /// Child nodes
        #[serde(default)]
        children: Vec<VNode>,
    },
    /// Empty node (renders nothing)
    #[default]
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
    pub fn attr<S: Into<String>, V: Into<AttrValue>>(mut self, name: S, value: V) -> Self {
        if let Self::Element { attrs, .. } = &mut self {
            attrs.insert(name.into(), value.into());
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

impl From<String> for AttrValue {
    fn from(s: String) -> Self {
        AttrValue::String(s)
    }
}

impl From<&str> for AttrValue {
    fn from(s: &str) -> Self {
        AttrValue::String(s.to_string())
    }
}

impl From<bool> for AttrValue {
    fn from(b: bool) -> Self {
        AttrValue::Bool(b)
    }
}

impl From<f64> for AttrValue {
    fn from(n: f64) -> Self {
        AttrValue::Number(n)
    }
}

impl From<usize> for AttrValue {
    fn from(n: usize) -> Self {
        AttrValue::Number(n as f64)
    }
}

impl From<u32> for AttrValue {
    fn from(n: u32) -> Self {
        AttrValue::Number(n as f64)
    }
}

impl From<i32> for AttrValue {
    fn from(n: i32) -> Self {
        AttrValue::Number(n as f64)
    }
}

/// Trait for types that can be converted into VNodes for the html! macro
pub trait IntoVNode {
    fn into_vnode(self) -> VNode;
}

impl IntoVNode for VNode {
    fn into_vnode(self) -> VNode { self }
}

impl IntoVNode for Option<VNode> {
    fn into_vnode(self) -> VNode {
        self.unwrap_or_else(VNode::empty)
    }
}

impl IntoVNode for String {
    fn into_vnode(self) -> VNode {
        VNode::text(self)
    }
}

impl IntoVNode for &String {
    fn into_vnode(self) -> VNode {
        VNode::text(self.clone())
    }
}

impl IntoVNode for &str {
    fn into_vnode(self) -> VNode {
        VNode::text(self.to_string())
    }
}

impl IntoVNode for Vec<VNode> {
    fn into_vnode(self) -> VNode {
        VNode::fragment(self)
    }
}

impl IntoVNode for f64 {
    fn into_vnode(self) -> VNode {
        VNode::text(self.to_string())
    }
}

impl IntoVNode for f32 {
    fn into_vnode(self) -> VNode {
        VNode::text(self.to_string())
    }
}

impl IntoVNode for i64 {
    fn into_vnode(self) -> VNode {
        VNode::text(self.to_string())
    }
}

impl IntoVNode for i32 {
    fn into_vnode(self) -> VNode {
        VNode::text(self.to_string())
    }
}

impl IntoVNode for i16 {
    fn into_vnode(self) -> VNode {
        VNode::text(self.to_string())
    }
}

impl IntoVNode for i8 {
    fn into_vnode(self) -> VNode {
        VNode::text(self.to_string())
    }
}

impl IntoVNode for u64 {
    fn into_vnode(self) -> VNode {
        VNode::text(self.to_string())
    }
}

impl IntoVNode for u32 {
    fn into_vnode(self) -> VNode {
        VNode::text(self.to_string())
    }
}

impl IntoVNode for u16 {
    fn into_vnode(self) -> VNode {
        VNode::text(self.to_string())
    }
}

impl IntoVNode for u8 {
    fn into_vnode(self) -> VNode {
        VNode::text(self.to_string())
    }
}

impl IntoVNode for usize {
    fn into_vnode(self) -> VNode {
        VNode::text(self.to_string())
    }
}

impl IntoVNode for isize {
    fn into_vnode(self) -> VNode {
        VNode::text(self.to_string())
    }
}

impl IntoVNode for bool {
    fn into_vnode(self) -> VNode {
        VNode::text(self.to_string())
    }
}

/// Helper to convert a value into a VNode
pub fn into_vnode<T: IntoVNode>(value: T) -> VNode {
    value.into_vnode()
}

/// Axum response support for VNode
impl axum::response::IntoResponse for VNode {
    fn into_response(self) -> axum::response::Response {
        let html = to_html(&self);
        axum::response::Response::builder()
            .header("Content-Type", "text/html; charset=utf-8")
            .body(axum::body::Body::from(html))
            .unwrap()
    }
}

/// Map React-style attribute names to HTML attribute names
fn map_attr_name(name: &str) -> &str {
    match name {
        "class_name" => "class",
        "for_id" => "for",
        "html_for" => "for",
        "tab_index" => "tabindex",
        "read_only" => "readonly",
        "max_length" => "maxlength",
        "auto_focus" => "autofocus",
        "auto_complete" => "autocomplete",
        "content_editable" => "contenteditable",
        "cross_origin" => "crossorigin",
        "http_equiv" => "http-equiv",
        "no_validate" => "novalidate",
        "form_action" => "formaction",
        "form_enc_type" => "formenctype",
        "form_method" => "formmethod",
        "form_no_validate" => "formnovalidate",
        "form_target" => "formtarget",
        "use_map" => "usemap",
        "date_time" => "datetime",
        _ => name,
    }
}

/// Convert a VNode to HTML string
pub fn to_html(node: &VNode) -> String {
    match node {
        VNode::Empty => String::new(),
        VNode::Text { value } => escape_html(value),
        VNode::Fragment { children } => children.iter().map(to_html).collect(),
        VNode::Component { name, children, .. } => {
            // For SSR, try to render through component registry if available,
            // otherwise render children as fallback.
            let children_html: String = children.iter().map(to_html).collect();
            if let Some(rendered) = try_render_component(name, children) {
                rendered
            } else {
                format!("<!-- {} -->{}", escape_html(name), children_html)
            }
        }
        VNode::Element { tag, attrs, children, .. } => {
            let mut html = format!("<{}", tag);
            
            // Add attributes
            for (name, value) in attrs {
                let html_name = map_attr_name(name);
                match value {
                    AttrValue::String(s) => {
                        html.push_str(&format!(" {}=\"{}\"", html_name, escape_attr(s)));
                    }
                    AttrValue::Bool(true) => {
                        html.push_str(&format!(" {}=\"{}\"", html_name, html_name));
                    }
                    AttrValue::Bool(false) => {
                        // Skip false booleans
                    }
                    AttrValue::Number(n) => {
                        html.push_str(&format!(" {}=\"{}\"", html_name, n));
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

use std::sync::Mutex;

lazy_static::lazy_static! {
    static ref COMPONENT_REGISTRY: Mutex<std::collections::HashMap<String, Box<dyn Fn(&std::collections::HashMap<String, serde_json::Value>, &[VNode]) -> Option<VNode> + Send + Sync>>> =
        Mutex::new(std::collections::HashMap::new());
}

/// Register a component for SSR rendering.
/// The callback receives props and children, and should return a VNode.
pub fn register_component<F>(name: &str, renderer: F)
where
    F: Fn(&std::collections::HashMap<String, serde_json::Value>, &[VNode]) -> Option<VNode> + Send + Sync + 'static,
{
    let mut reg = COMPONENT_REGISTRY.lock().unwrap();
    reg.insert(name.to_string(), Box::new(renderer));
}

/// Try to render a registered component. Returns None if not registered.
fn try_render_component(name: &str, children: &[VNode]) -> Option<String> {
    let reg = COMPONENT_REGISTRY.lock().unwrap();
    let renderer = reg.get(name)?;
    // For SSR without actual props lookup, we pass empty props.
    // In practice, components that need SSR should be inlined by codegen
    // or the registry should be populated at startup with prop-aware closures.
    let vnode = renderer(&std::collections::HashMap::new(), children)?;
    Some(to_html(&vnode))
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
