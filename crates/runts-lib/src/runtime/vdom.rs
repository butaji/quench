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
            Self::Text { value } => value.clone(),
            Self::Element {
                tag,
                attrs,
                children,
                ..
            } => {
                let attrs_str = attrs
                    .iter()
                    .map(|(k, v)| format!(r#"{}="{}""#, k, v))
                    .collect::<Vec<_>>()
                    .join(" ");
                let children_html = children.iter().map(|c| c.to_html()).collect::<String>();
                if children_html.is_empty() {
                    format!("<{} {} />", tag, attrs_str)
                } else {
                    format!("<{} {}>{}</{}>", tag, attrs_str, children_html, tag)
                }
            }
            Self::Component { name: _, children, .. } => {
                children.iter().map(|c| c.to_html()).collect()
            }
            Self::Fragment { children } => children.iter().map(|c| c.to_html()).collect(),
        }
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
