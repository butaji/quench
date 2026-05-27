//! HTML builder helpers

use std::collections::HashMap;
use crate::runtime::vdom::{VNode, AttrValue};

/// Build an HTML element
#[allow(dead_code)]
pub fn html_element(tag: &str, attrs: HashMap<String, serde_json::Value>, children: Vec<VNode>) -> VNode {
    let mut vdom_attrs = HashMap::new();
    for (key, value) in attrs {
        let attr = match value {
            serde_json::Value::String(s) => AttrValue::String(s),
            serde_json::Value::Bool(b) => AttrValue::Bool(b),
            serde_json::Value::Number(n) => AttrValue::Number(n.as_f64().unwrap_or(0.0)),
            _ => AttrValue::String(value.to_string()),
        };
        vdom_attrs.insert(key, attr);
    }
    
    VNode::Element {
        tag: tag.to_string(),
        attrs: vdom_attrs,
        events: HashMap::new(),
        children,
        key: None,
    }
}

/// Create a text node
pub fn text(value: impl ToString) -> VNode {
    VNode::Text {
        value: value.to_string(),
    }
}

/// Create a fragment
#[allow(dead_code)]
pub fn fragment(children: Vec<VNode>) -> VNode {
    VNode::Fragment(children)
}

/// Create an empty node
pub fn empty() -> VNode {
    VNode::Empty
}
