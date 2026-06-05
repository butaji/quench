//! Virtual DOM implementation

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AttrValue {
    String(String),
    Bool(bool),
    Number(f64),
}
impl std::fmt::Display for AttrValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String(s) => write!(f, "{}", s),
            Self::Bool(b) => write!(f, "{}", b),
            Self::Number(n) => write!(f, "{}", n),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum VNode {
    #[default]
    Empty,
    Text {
        value: String,
    },
    Element {
        tag: String,
        attrs: HashMap<String, AttrValue>,
        #[serde(default)]
        events: HashMap<String, String>,
        #[serde(default)]
        children: Vec<VNode>,
        #[serde(skip_serializing_if = "Option::is_none")]
        key: Option<String>,
    },
    Component {
        name: String,
        #[serde(default)]
        props: HashMap<String, serde_json::Value>,
        #[serde(default)]
        children: Vec<VNode>,
    },
    Fragment {
        #[serde(default)]
        children: Vec<VNode>,
    },
}

impl VNode {
    pub fn empty() -> Self {
        Self::Empty
    }
    pub fn text<S: Into<String>>(value: S) -> Self {
        Self::Text {
            value: value.into(),
        }
    }
    pub fn element<S: Into<String>>(tag: S) -> Self {
        Self::Element {
            tag: tag.into(),
            attrs: HashMap::new(),
            events: HashMap::new(),
            children: Vec::new(),
            key: None,
        }
    }
    pub fn fragment(children: Vec<VNode>) -> Self {
        Self::Fragment { children }
    }
    pub fn attr<S: Into<String>, V: Into<AttrValue>>(mut self, name: S, value: V) -> Self {
        if let Self::Element { attrs, .. } = &mut self {
            attrs.insert(name.into(), value.into());
        }
        self
    }
    pub fn child(mut self, child: VNode) -> Self {
        if let Self::Element { children, .. } = &mut self {
            children.push(child);
        }
        self
    }
    pub fn to_html(&self) -> String {
        match self {
            Self::Empty => String::new(),
            Self::Text { value } => escape_html(value),
            Self::Element { tag, attrs, children, .. } => {
                let is_void = is_void_element(tag);
                let attrs_str = format_attrs(attrs);
                let children_html: String = children.iter().map(|c| c.to_html()).collect::<String>();
                render_element(tag, &attrs_str, &children_html, is_void)
            }
            Self::Component { name: _, children, .. } => {
                children.iter().map(|c| c.to_html()).collect()
            }
            Self::Fragment { children } => children.iter().map(|c| c.to_html()).collect(),
        }
    }
}

fn is_void_element(tag: &str) -> bool {
    matches!(tag, "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input" | "link" | "meta" | "param" | "source" | "track" | "wbr")
}

fn format_attrs(attrs: &HashMap<String, AttrValue>) -> String {
    let parts: Vec<String> = attrs
        .iter()
        .filter_map(|(k, v)| match v {
            AttrValue::Bool(true) => Some(k.clone()),
            AttrValue::Bool(false) => None,
            _ => Some(format!(r#"{}="{}""#, k, escape_html_attr(v))),
        })
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!(" {}", parts.join(" "))
    }
}

fn render_element(tag: &str, attrs_str: &str, children_html: &str, is_void: bool) -> String {
    if children_html.is_empty() && is_void {
        format!("<{}{} />", tag, attrs_str)
    } else if children_html.is_empty() {
        format!("<{}{}></{}>", tag, attrs_str, tag)
    } else {
        format!("<{}{}>{}</{}>", tag, attrs_str, children_html, tag)
    }
}

impl From<String> for AttrValue {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}
impl From<&str> for AttrValue {
    fn from(s: &str) -> Self {
        Self::String(s.to_string())
    }
}
impl From<bool> for AttrValue {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}
impl From<f64> for AttrValue {
    fn from(n: f64) -> Self {
        Self::Number(n)
    }
}
impl From<i32> for AttrValue {
    fn from(n: i32) -> Self {
        Self::Number(n as f64)
    }
}
impl From<i64> for AttrValue {
    fn from(n: i64) -> Self {
        Self::Number(n as f64)
    }
}
impl From<u32> for AttrValue {
    fn from(n: u32) -> Self {
        Self::Number(n as f64)
    }
}
impl From<u64> for AttrValue {
    fn from(n: u64) -> Self {
        Self::Number(n as f64)
    }
}
impl From<usize> for AttrValue {
    fn from(n: usize) -> Self {
        Self::Number(n as f64)
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn escape_html_attr(v: &AttrValue) -> String {
    match v {
        AttrValue::String(s) => s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#x27;"),
        AttrValue::Number(n) => n.to_string(),
        AttrValue::Bool(b) => b.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vnode_empty() {
        let node = VNode::empty();
        assert_eq!(node, VNode::Empty);
        assert_eq!(node.to_html(), "");
    }

    #[test]
    fn test_vnode_text() {
        let node = VNode::text("hello");
        assert_eq!(node, VNode::Text { value: "hello".to_string() });
        assert_eq!(node.to_html(), "hello");
    }

    #[test]
    fn test_vnode_text_special_chars() {
        let node = VNode::text("<script>&\"'");
        assert_eq!(node.to_html(), "&lt;script&gt;&amp;&quot;&#x27;");
    }

    #[test]
    fn test_vnode_element_basic() {
        let node = VNode::element("div");
        assert!(matches!(node, VNode::Element { ref tag, .. } if tag == "div"));
        assert_eq!(node.to_html(), "<div></div>");
    }

    #[test]
    fn test_vnode_element_with_attrs() {
        let node = VNode::element("div")
            .attr("class", "container")
            .attr("id", "main");
        let html = node.to_html();
        // HashMap ordering is non-deterministic, just check both attrs present
        assert!(html.contains(r#"class="container""#));
        assert!(html.contains(r#"id="main""#));
    }

    #[test]
    fn test_vnode_element_with_bool_attr() {
        let node = VNode::element("input")
            .attr("disabled", true)
            .attr("readonly", false);
        assert_eq!(node.to_html(), r#"<input disabled />"#);
    }

    #[test]
    fn test_vnode_element_with_numeric_attr() {
        let node = VNode::element("input")
            .attr("maxlength", 100i32);
        assert_eq!(node.to_html(), r#"<input maxlength="100" />"#);
    }

    #[test]
    fn test_vnode_element_with_children() {
        let child = VNode::text("hello");
        let node = VNode::element("div")
            .child(VNode::element("span").child(child));
        assert_eq!(node.to_html(), "<div><span>hello</span></div>");
    }

    #[test]
    fn test_vnode_fragment() {
        let node = VNode::fragment(vec![
            VNode::text("a"),
            VNode::text("b"),
        ]);
        assert_eq!(node.to_html(), "ab");
    }

    #[test]
    fn test_vnode_component() {
        let node = VNode::Component {
            name: "Button".to_string(),
            props: HashMap::new(),
            children: vec![VNode::text("Click")],
        };
        assert_eq!(node.to_html(), "Click");
    }

    #[test]
    fn test_vnode_void_elements() {
        let tags = ["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];
        for tag in tags {
            let node = VNode::element(tag);
            assert_eq!(node.to_html(), format!("<{} />", tag));
        }
    }

    #[test]
    fn test_attr_value_display() {
        assert_eq!(AttrValue::String("hello".to_string()).to_string(), "hello");
        assert_eq!(AttrValue::Bool(true).to_string(), "true");
        assert_eq!(AttrValue::Bool(false).to_string(), "false");
        assert_eq!(AttrValue::Number(42.0).to_string(), "42");
    }

    #[test]
    fn test_attr_value_from_types() {
        use std::collections::HashMap;
        let mut attrs = HashMap::new();
        attrs.insert("str".to_string(), AttrValue::from("hello"));
        attrs.insert("bool".to_string(), AttrValue::from(true));
        attrs.insert("f64".to_string(), AttrValue::from(3.14));
        attrs.insert("i32".to_string(), AttrValue::from(42i32));
        attrs.insert("i64".to_string(), AttrValue::from(42i64));
        attrs.insert("u32".to_string(), AttrValue::from(42u32));
        attrs.insert("u64".to_string(), AttrValue::from(42u64));
        attrs.insert("usize".to_string(), AttrValue::from(42usize));

        assert!(matches!(attrs.get("str"), Some(AttrValue::String(s)) if s == "hello"));
        assert!(matches!(attrs.get("bool"), Some(AttrValue::Bool(true))));
        assert!(matches!(attrs.get("f64"), Some(AttrValue::Number(n)) if (*n - 3.14).abs() < 0.001));
        assert!(matches!(attrs.get("i32"), Some(AttrValue::Number(n)) if *n == 42.0));
    }

    #[test]
    fn test_vnode_serde_roundtrip() {
        let node = VNode::element("div")
            .attr("class", "test")
            .child(VNode::text("content"));

        let json = serde_json::to_string(&node).unwrap();
        let decoded: VNode = serde_json::from_str(&json).unwrap();
        assert_eq!(node, decoded);
    }

    #[test]
    fn test_attr_value_serde_roundtrip() {
        let val = AttrValue::String("hello".to_string());
        let json = serde_json::to_string(&val).unwrap();
        let decoded: AttrValue = serde_json::from_str(&json).unwrap();
        assert_eq!(val, decoded);

        let val2 = AttrValue::Number(42.0);
        let json2 = serde_json::to_string(&val2).unwrap();
        let decoded2: AttrValue = serde_json::from_str(&json2).unwrap();
        assert_eq!(val2, decoded2);
    }

    #[test]
    fn test_vnode_clone() {
        let node = VNode::element("div").attr("id", "test");
        let cloned = node.clone();
        assert_eq!(node, cloned);
    }

    #[test]
    fn test_vnode_partial_eq() {
        let node1 = VNode::element("div");
        let node2 = VNode::element("div");
        let node3 = VNode::element("span");
        assert_eq!(node1, node2);
        assert_ne!(node1, node3);
    }

    #[test]
    fn test_vnode_key_serde() {
        let node = VNode::Element {
            tag: "li".to_string(),
            attrs: HashMap::new(),
            events: HashMap::new(),
            children: vec![],
            key: Some("item-1".to_string()),
        };
        let json = serde_json::to_string(&node).unwrap();
        assert!(json.contains("\"key\":\"item-1\""));
        let decoded: VNode = serde_json::from_str(&json).unwrap();
        assert_eq!(node, decoded);
    }
}
