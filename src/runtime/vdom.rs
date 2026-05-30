//! Virtual DOM / Fine-grained rendering system
//!
//! This module provides the core rendering primitives for runts.
//! Instead of full VDOM diffing, we use a hybrid approach:
//! - Static HTML is generated directly (no diffing)
//! - Islands use fine-grained signals for updates
//! - Client-side hydration connects signals to DOM
//!
//! # Deprecation Notice
//!
//! **This file is DEPRECATED.** The canonical vdom implementation lives at:
//! `crates/runts-lib/src/runtime/vdom.rs`
//!
//! This file exists for backwards compatibility but should not be used by plugins.
//! It may be removed in a future version.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Virtual Node Key - used for list reconciliation
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Key(pub String);

#[allow(dead_code)]
impl Key {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl From<String> for Key {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for Key {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Attribute value
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttrValue {
    String(String),
    Bool(bool),
    Number(f64),
}

/// Event handler type
pub type EventHandler = Box<dyn Fn(JsValue) + Send + Sync>;

/// Virtual Node - represents a rendered element
///
/// Note: We don't derive Debug or Clone because EventHandler doesn't implement them.
/// For SSR, we use Render trait instead.
#[allow(dead_code)]
pub enum VNode {
    /// HTML/SVG element
    Element {
        /// Tag name (e.g., "div", "span", "p")
        tag: String,

        /// Attributes (e.g., class, id, data-*)
        /// For boolean attributes like `disabled`, use `AttrValue::Bool(true)`
        attrs: HashMap<String, AttrValue>,

        /// Event handlers (e.g., on_click, on_input)
        /// Stored as (event_name, handler) tuples
        events: HashMap<String, EventHandler>,

        /// Child nodes
        children: Vec<VNode>,

        /// Key for list reconciliation
        key: Option<Key>,
    },

    /// Component instance
    Component {
        /// Component function/type name
        name: String,

        /// Props passed to the component
        props: HashMap<String, serde_json::Value>,

        /// Children passed as props.children
        children: Vec<VNode>,

        /// Key for list reconciliation
        key: Option<Key>,
    },

    /// Text content
    Text {
        /// The text value
        value: String,
    },

    /// Fragment - group of nodes without a wrapper
    Fragment(Vec<VNode>),

    /// Empty node (renders nothing)
    Empty,
}

// Note: VNode doesn't implement Clone because EventHandler doesn't implement Clone.
// For SSR, we use Render trait instead.

/// Create a VNode from an element
#[allow(dead_code)]
impl VNode {
    /// Create a text VNode
    pub fn text(value: impl Into<String>) -> Self {
        VNode::Text {
            value: value.into(),
        }
    }

    /// Create an element VNode
    pub fn element(tag: impl Into<String>) -> ElementBuilder {
        ElementBuilder {
            tag: tag.into(),
            attrs: HashMap::new(),
            events: HashMap::new(),
            children: Vec::new(),
            key: None,
        }
    }
}

/// Builder pattern for element VNodes
#[allow(dead_code)]
pub struct ElementBuilder {
    tag: String,
    attrs: HashMap<String, AttrValue>,
    events: HashMap<String, EventHandler>,
    children: Vec<VNode>,
    key: Option<Key>,
}

#[allow(dead_code)]
impl ElementBuilder {
    /// Add an attribute
    pub fn attr(mut self, name: impl Into<String>, value: impl Into<AttrValue>) -> Self {
        self.attrs.insert(name.into(), value.into());
        self
    }

    /// Add a class (convenience method)
    pub fn class(mut self, class: impl Into<String>) -> Self {
        let class = class.into();
        self.attrs
            .entry("class".to_string())
            .and_modify(|e| {
                if let AttrValue::String(ref mut s) = e {
                    *s = format!("{} {}", s, class);
                }
            })
            .or_insert_with(|| AttrValue::String(class));
        self
    }

    /// Add an event handler
    pub fn on(mut self, event: impl Into<String>, handler: EventHandler) -> Self {
        self.events.insert(event.into(), handler);
        self
    }

    /// Add a child node
    pub fn child(mut self, child: VNode) -> Self {
        self.children.push(child);
        self
    }

    /// Add multiple children
    pub fn children(mut self, children: impl IntoIterator<Item = VNode>) -> Self {
        self.children.extend(children);
        self
    }

    /// Set the key
    pub fn key(mut self, key: Key) -> Self {
        self.key = Some(key);
        self
    }

    /// Build the VNode
    pub fn build(self) -> VNode {
        VNode::Element {
            tag: self.tag,
            attrs: self.attrs,
            events: self.events,
            children: self.children,
            key: self.key,
        }
    }
}

impl AttrValue {
    /// Convert to HTML attribute string
    pub fn to_html_attr(&self, name: &str) -> String {
        match self {
            AttrValue::String(s) => format!("{}=\"{}\"", name, s),
            AttrValue::Bool(true) => name.to_string(),
            AttrValue::Bool(false) => String::new(),
            AttrValue::Number(n) => format!("{}=\"{}\"", name, n),
        }
    }
}

/// Trait for types that can be rendered
#[allow(dead_code)]
pub trait Render {
    /// Render to HTML string (for SSR)
    fn render_to_html(&self) -> String;

    /// Render to HTML string with indentation
    fn render_to_html_indented(&self, _indent: usize) -> String {
        self.render_to_html()
    }
}

impl Render for VNode {
    fn render_to_html(&self) -> String {
        match self {
            VNode::Element {
                tag,
                attrs,
                events: _,
                children,
                key: _,
            } => render_element(tag, attrs, children),
            VNode::Component {
                name: _,
                props: _,
                children,
                key: _,
            } => render_component_children(children),
            VNode::Text { value } => html_escape(value),
            VNode::Fragment(children) => render_children(children),
            VNode::Empty => String::new(),
        }
    }
}

fn render_element(tag: &str, attrs: &HashMap<String, AttrValue>, children: &[VNode]) -> String {
    let attr_str = attrs
        .iter()
        .map(|(name, value)| value.to_html_attr(name))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let opening = if attr_str.is_empty() {
        format!("<{}>", tag)
    } else {
        format!("<{} {}>", tag, attr_str)
    };
    let children_html = render_children(children);
    format!("{}</{}>", opening + &children_html, tag)
}

fn render_component_children(children: &[VNode]) -> String {
    // Components are rendered server-side by their children.
    // The actual component function is invoked at build time
    // to produce the VNode tree; this runtime path only
    // renders the already-resolved children.
    render_children(children)
}

fn render_children(children: &[VNode]) -> String {
    children.iter().map(|c| c.render_to_html()).collect()
}

/// Escape HTML special characters
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Fragment - a group of nodes without a wrapper element
pub struct Fragment {
    children: Vec<VNode>,
}

#[allow(dead_code)]
impl Fragment {
    pub fn new(children: Vec<VNode>) -> Self {
        Self { children }
    }

    pub fn from_iter<I: IntoIterator<Item = VNode>>(children: I) -> Self {
        Self {
            children: children.into_iter().collect(),
        }
    }
}

impl Render for Fragment {
    fn render_to_html(&self) -> String {
        self.children.iter().map(|c| c.render_to_html()).collect()
    }
}

/// Trait for types that can be used as children in components
#[allow(dead_code)]
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
        VNode::Text {
            value: self.to_string(),
        }
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

/// Convert a value into a VNode
#[allow(dead_code)]
pub fn into_vnode<T: IntoVNode>(value: T) -> VNode {
    value.into_vnode()
}

/// Axum response support for VNode
impl axum::response::IntoResponse for VNode {
    fn into_response(self) -> axum::response::Response {
        let html = self.render_to_html();
        axum::response::Response::builder()
            .header("Content-Type", "text/html; charset=utf-8")
            .body(axum::body::Body::from(html))
            .unwrap()
    }
}

/// Macro for creating VNodes with HTML-like syntax
///
/// # Example
/// ```ignore
/// html! {
///     <div class="container">
///         <h1>{ "Hello" }</h1>
///         <p>{ message }</p>
///     </div>
/// }
/// ```
///
/// Note: This is a simplified version. Full macro implementation
/// would require procedural macros for proper JSX-like syntax.
/// For production use, use the `#[component]` proc macro with proper html! syntax.
#[macro_export]
macro_rules! html {
    // Text node
    ($text:expr) => {
        $crate::runtime::vdom::VNode::Text {
            value: $text.to_string(),
        }
    };
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

impl From<i32> for AttrValue {
    fn from(n: i32) -> Self {
        AttrValue::Number(n as f64)
    }
}

impl From<f64> for AttrValue {
    fn from(n: f64) -> Self {
        AttrValue::Number(n)
    }
}

/// Type for JavaScript values (placeholder for actual JS interop)
#[derive(Debug, Clone)]
pub struct JsValue;

#[allow(dead_code)]
impl JsValue {
    pub fn null() -> Self {
        JsValue
    }

    pub fn undefined() -> Self {
        JsValue
    }

    pub fn string(s: impl Into<String>) -> Self {
        let _ = s.into();
        JsValue
    }

    pub fn number(n: f64) -> Self {
        let _ = n;
        JsValue
    }

    pub fn bool(b: bool) -> Self {
        let _ = b;
        JsValue
    }
}

/// Placeholder for web_sys types in server context
#[allow(dead_code)]
pub mod web_sys {
    pub struct Event;
    pub struct MouseEvent;
    pub struct InputEvent;
    pub struct KeyboardEvent;
    pub struct FocusEvent;
    pub struct SubmitEvent;
    pub struct ChangeEvent;
}
