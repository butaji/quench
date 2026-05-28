//! Rendering utilities for interpreter

use super::*;

/// Render a value to HTML string
pub fn render_value(value: &Value) -> String {
    match value {
        Value::Null | Value::Undefined => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => html_escape(s),
        Value::Array(arr) => arr.iter().map(render_value).collect(),
        Value::Object(map) => {
            let inner: String = map.values().map(render_value).collect();
            format!("<span>{}</span>", inner)
        }
        Value::VNode(vnode) => render_vnode(vnode),
        Value::Function(_) => String::new(),
    }
}

fn render_vnode(vnode: &VNodeValue) -> String {
    let attrs: String = vnode.attrs.iter()
        .map(|(k, v)| format!(" {}=\"{}\"", k, render_value(v)))
        .collect();

    let children: String = vnode.children.iter()
        .map(render_value)
        .collect();

    if children.is_empty() {
        format!("<{}{} />", vnode.tag, attrs)
    } else {
        format!("<{}{}>{}</{}>", vnode.tag, attrs, children, vnode.tag)
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
