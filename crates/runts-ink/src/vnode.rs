//! VNode tree used by the Ink-style component model.
//!
//! A VNode is either a `Box` (a flexbox-style container),
//! a `Text` (a string with optional colour / weight), a
//! `Newline` (vertical separator), a `Spacer` (a flexbox
//! separator that fills remaining space along the main
//! axis), a `Static` (pre-rendered fragment that doesn't
//! re-flow), a `Transform` (offset the wrapped child), or
//! a `Fragment` (a list of children with no wrapper).
//!
//! The tree is plain data — no `Box<dyn Any>` tricks, no
//! trait objects. Each variant holds its own children.
//! This makes it cheap to clone and easy to walk during
//! render.

use serde::{Deserialize, Serialize};

use crate::components::{
    Box, Newline, Spacer, Static, Text, Transform,
};

/// A VNode variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VNodeContent {
    /// A flexbox-style container.
    Box(Box),
    /// A single line of text.
    Text(Text),
    /// A vertical separator (renders as a blank line).
    Newline(Newline),
    /// A flexbox separator (fills the remaining main-axis space).
    Spacer(Spacer),
    /// A pre-rendered fragment.
    Static(Static),
    /// A child wrapped in an offset.
    Transform(Transform),
    /// A list of children with no wrapper.
    Fragment(Vec<VNode>),
}

/// A node in the VNode tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VNode(pub VNodeContent);

impl VNode {
    /// The kind of this VNode.
    pub fn kind(&self) -> &'static str {
        match &self.0 {
            VNodeContent::Box(_) => "box",
            VNodeContent::Text(_) => "text",
            VNodeContent::Newline(_) => "newline",
            VNodeContent::Spacer(_) => "spacer",
            VNodeContent::Static(_) => "static",
            VNodeContent::Transform(_) => "transform",
            VNodeContent::Fragment(_) => "fragment",
        }
    }

    /// Children of this VNode. Empty for leaves.
    pub fn children(&self) -> &[VNode] {
        match &self.0 {
            VNodeContent::Box(b) => &b.children,
            VNodeContent::Text(_) => &[],
            VNodeContent::Newline(_) => &[],
            VNodeContent::Spacer(_) => &[],
            VNodeContent::Static(s) => &s.children,
            VNodeContent::Transform(t) => std::slice::from_ref(&t.child),
            VNodeContent::Fragment(fs) => fs.as_slice(),
        }
    }
}

impl From<VNodeContent> for VNode {
    fn from(content: VNodeContent) -> Self {
        VNode(content)
    }
}

impl From<Text> for VNode {
    fn from(text: Text) -> Self {
        VNode(VNodeContent::Text(text))
    }
}

impl From<Box> for VNode {
    fn from(b: Box) -> Self {
        VNode(VNodeContent::Box(b))
    }
}

impl From<Newline> for VNode {
    fn from(n: Newline) -> Self {
        VNode(VNodeContent::Newline(n))
    }
}

impl From<Spacer> for VNode {
    fn from(s: Spacer) -> Self {
        VNode(VNodeContent::Spacer(s))
    }
}

impl From<Static> for VNode {
    fn from(s: Static) -> Self {
        VNode(VNodeContent::Static(s))
    }
}

impl From<Transform> for VNode {
    fn from(t: Transform) -> Self {
        VNode(VNodeContent::Transform(t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_reports_box_for_box() {
        let v = VNode::from(Box::column());
        assert_eq!(v.kind(), "box");
    }

    #[test]
    fn kind_reports_text_for_text() {
        let v = VNode::from(Text::new("hi"));
        assert_eq!(v.kind(), "text");
    }

    #[test]
    fn kind_reports_fragment() {
        let v = VNode::from(VNodeContent::Fragment(vec![]));
        assert_eq!(v.kind(), "fragment");
    }

    #[test]
    fn children_empty_for_text() {
        let v = VNode::from(Text::new("hi"));
        assert!(v.children().is_empty());
    }

    #[test]
    fn children_returns_inner_for_box() {
        let v = VNode::from(Box::column().child(Text::new("a")).child(Text::new("b")));
        assert_eq!(v.children().len(), 2);
    }

    #[test]
    fn children_returns_inner_for_static() {
        let v = VNode::from(Static::new().child(Text::new("x")));
        assert_eq!(v.children().len(), 1);
    }

    #[test]
    fn children_returns_single_for_transform() {
        let v = VNode::from(Transform::new(Text::new("x")));
        assert_eq!(v.children().len(), 1);
    }

    #[test]
    fn children_returns_inner_for_fragment() {
        let v = VNode::from(VNodeContent::Fragment(vec![
            VNode::from(Text::new("a")),
            VNode::from(Text::new("b")),
        ]));
        assert_eq!(v.children().len(), 2);
    }
}
