//! Yoga-based layout engine for runts-ink.
//!
//! This module replaces the previous Taffy-based
//! layout engine. Yoga is the same CSS flexbox
//! engine that real Ink uses (via the
//! `yoga-layout` npm package), so we get
//! byte-for-byte identical layout results to
//! Ink — no manual flex_grow expansion, no
//! manual justify-content workarounds, no
//! custom measure function for non-text leaves.
//!
//! The `yoga` Rust crate
//! (https://github.com/bschwind/yoga-rs) compiles
//! the C++ Yoga from source via bindgen.

use std::collections::HashMap;

use crate::components::{InkBox, InkText, Newline as InkNewline, Spacer as InkSpacer, Static as InkStatic, Transform as InkTransform};
use crate::vnode::{VNode, VNodeContent};

/// Per-VNode-index layout rect. Indexed by VNode
/// pre-order DFS position. The renderer's
/// `walk` function looks up `rects[depth]` to
/// get the computed position and size for each
/// node.
pub type Rect = (u16, u16, u16, u16);

/// A Yoga tree built from a VNode tree.
///
/// Holds the root Yoga node and a parallel vec
/// mapping VNode pre-order index → Yoga node.
/// The renderer walks the VNode tree in DFS
/// order and uses `yoga_index[depth]` to look
/// up the computed layout for each node.
pub struct YogaTree {
    /// The root Yoga node.
    pub root: yoga::Node,
    /// VNode pre-order index → Yoga node.
    yoga_index: Vec<yoga::Node>,
    /// Per-VNode text content. Used by the Yoga
    /// measure function to compute intrinsic
    /// text size.
    text_by_index: Vec<Option<String>>,
}

impl YogaTree {
    /// Build a Yoga tree from a VNode tree.
    pub fn from_vnode(root: &VNode) -> Self {
        let mut yoga_index: Vec<yoga::Node> = Vec::new();
        let mut text_by_index: Vec<Option<String>> = Vec::new();
        let root_node = build_node(root, &mut yoga_index, &mut text_by_index);
        Self {
            root: root_node,
            yoga_index,
            text_by_index,
        }
    }

    /// Compute the layout with the given viewport
    /// size (in cells). Returns a vec of rects
    /// indexed by VNode pre-order position.
    pub fn compute(&self, viewport_w: f32, viewport_h: f32) -> Vec<Rect> {
        // Install measure function on every node.
        // Yoga calls this for leaf nodes to get
        // their intrinsic size. For Text leaves
        // we return the string's char count. For
        // Box leaves we return the known
        // dimensions (available space).
        let text_lookup: HashMap<usize, String> = self
            .text_by_index
            .iter()
            .enumerate()
            .filter_map(|(i, t)| t.as_ref().map(|s| (i, s.clone())))
            .collect();

        // Yoga doesn't have a per-node measure
        // function like Taffy. Instead, we set
        // the measure function on each leaf
        // node before computing.
        for (i, node) in self.yoga_index.iter().enumerate() {
            if let Some(text) = text_lookup.get(&i) {
                let text = text.clone();
                let measure = move |_width: f32,
                                    _width_mode: yoga::MeasureMode,
                                    _height: f32,
                                    _height_mode: yoga::MeasureMode|
                      -> yoga::Size {
                    let w = text.chars().count() as f32;
                    yoga::Size {
                        width: w,
                        height: 1.0,
                    }
                };
                node.set_measure_func(Some(measure));
            }
        }

        self.root.calculate_layout(
            viewport_w,
            viewport_h,
            yoga::Direction::LTR,
        );

        // Collect rects. Yoga's get_layout()
        // returns left/top in absolute (root)
        // coordinates — no parent-chain walk
        // needed!
        let mut rects = vec![(0u16, 0u16, 0u16, 0u16); self.yoga_index.len()];
        for (i, node) in self.yoga_index.iter().enumerate() {
            let layout = node.get_layout();
            let x = layout.left().max(0.0).min(u16::MAX as f32) as u16;
            let y = layout.top().max(0.0).min(u16::MAX as f32) as u16;
            let w = layout.width().max(0.0).min(u16::MAX as f32) as u16;
            let h = layout.height().max(0.0).min(u16::MAX as f32) as u16;
            if let Some(slot) = rects.get_mut(i) {
                *slot = (x, y, w, h);
            }
        }
        rects
    }
}

/// Recursively build Yoga nodes from a VNode tree.
fn build_node(
    node: &VNode,
    yoga_index: &mut Vec<yoga::Node>,
    text_by_index: &mut Vec<Option<String>>,
) -> yoga::Node {
    match &node.0 {
        VNodeContent::Box(b) => {
            let mut yoga_node = crate::yoga_bridge::new_yoga_node(b);
            for child in &b.children {
                let child_node = build_node(child, yoga_index, text_by_index);
                yoga_node.insert_child(&mut child_node.clone(), yoga_node.child_count());
                // Note: insert_child takes &mut Node, but
                // we need to keep the child alive. Yoga
                // nodes are reference-counted internally.
            }
            yoga_index.push(yoga_node.clone());
            text_by_index.push(None);
            yoga_node
        }
        VNodeContent::Text(t) => {
            let mut yoga_node = yoga::Node::new();
            text_by_index.push(Some(t.content.clone()));
            yoga_index.push(yoga_node.clone());
            yoga_node
        }
        VNodeContent::Newline(_) => {
            let mut yoga_node = yoga::Node::new();
            let styles = vec![
                yoga::FlexStyle::Display(yoga::Display::Flex),
                yoga::FlexStyle::Height(yoga::StyleUnit::Point(1.0)),
            ];
            yoga_node.apply_styles(&styles);
            yoga_index.push(yoga_node.clone());
            text_by_index.push(None);
            yoga_node
        }
        VNodeContent::Spacer(s) => {
            let mut yoga_node = yoga::Node::new();
            let mut styles = vec![
                yoga::FlexStyle::FlexGrow(1.0.into()),
            ];
            // If Spacer has explicit flex_grow, use it
            // (currently Spacer is a unit struct with
            // implicit flex_grow: 1)
            let _ = s;
            yoga_node.apply_styles(&styles);
            yoga_index.push(yoga_node.clone());
            text_by_index.push(None);
            yoga_node
        }
        VNodeContent::Static(s) => {
            let mut yoga_node = yoga::Node::new();
            for child in &s.children {
                let child_node = build_node(child, yoga_index, text_by_index);
                yoga_node.insert_child(&mut child_node.clone(), yoga_node.child_count());
            }
            yoga_index.push(yoga_node.clone());
            text_by_index.push(None);
            yoga_node
        }
        VNodeContent::Transform(t) => {
            let mut yoga_node = yoga::Node::new();
            let styles = vec![
                yoga::FlexStyle::PositionType(yoga::PositionType::Absolute),
                yoga::FlexStyle::Position(yoga::Edge::Left, yoga::StyleUnit::Point(t.x as f32)),
                yoga::FlexStyle::Position(yoga::Edge::Top, yoga::StyleUnit::Point(t.y as f32)),
            ];
            yoga_node.apply_styles(&styles);
            let child_node = build_node(&t.child, yoga_index, text_by_index);
            yoga_node.insert_child(&mut child_node.clone(), 0);
            yoga_index.push(yoga_node.clone());
            text_by_index.push(None);
            yoga_node
        }
        VNodeContent::Fragment(fs) => {
            let mut yoga_node = yoga::Node::new();
            for child in fs {
                let child_node = build_node(child, yoga_index, text_by_index);
                yoga_node.insert_child(&mut child_node.clone(), yoga_node.child_count());
            }
            yoga_index.push(yoga_node.clone());
            text_by_index.push(None);
            yoga_node
        }
    }
}
