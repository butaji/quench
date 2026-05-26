//! HTML builder helpers

use std::collections::HashMap;
use crate::runtime::vdom::VNode;

/// Build an HTML element
#[allow(dead_code)]
pub fn html_element(tag: &str, attrs: HashMap<String, serde_json::Value>, children: Vec<VNode>) -> VNode {
    // serde::Serialize is used indirectly through VNodeValue
    
    let mut vdom_attrs = HashMap::new();
    for (key, value) in attrs {
        // Convert JSON values to VNodeValue
        if let Ok(v) = serde_json::from_value::<crate::runtime::vdom::VNodeValue>(value.clone()) {
            vdom_attrs.insert(key, v);
        } else {
            vdom_attrs.insert(key, crate::runtime::vdom::VNodeValue::Null);
        }
    }
    
    VNode::Element {
        tag: tag.to_string(),
        attrs: vdom_attrs,
        children,
        events: HashMap::new(),
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
