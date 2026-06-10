//! Ink node representation
//!
//! Contains InkNode struct with Yoga layout and prop application.

// PropValue is defined in this file, don't re-export
use ordered_float::OrderedFloat;
use std::collections::HashMap;
use yoga::{Align, Display, FlexDirection, Justify, Node, PositionType, StyleUnit, Wrap};

/// Tag types matching React component names from Ink
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InkTag {
    Box,
    Text,
    Static,
    Newline,
    Spacer,
    Unknown,
}

impl InkTag {
    pub fn from_str(s: &str) -> Self {
        match s {
            "ink-box" => InkTag::Box,
            "ink-text" => InkTag::Text,
            "ink-static" => InkTag::Static,
            "ink-newline" => InkTag::Newline,
            "ink-spacer" => InkTag::Spacer,
            _ => InkTag::Unknown,
        }
    }
}

/// Property value types from JSX props
#[derive(Debug, Clone, PartialEq)]
pub enum PropValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Vec(Vec<PropValue>),
}

/// A single Ink node in the tree
pub struct InkNode {
    pub id: u32,
    pub tag: InkTag,
    pub props: HashMap<String, PropValue>,
    pub text: Option<String>,
    pub parent: Option<u32>,
    pub children: Vec<u32>,
    pub yoga: Node,
}

impl InkNode {
    pub fn new(id: u32, tag: InkTag) -> Self {
        let mut yoga = Node::new();
        yoga.set_flex_direction(FlexDirection::Row);
        yoga.set_align_items(Align::FlexStart);
        yoga.set_justify_content(Justify::FlexStart);

        Self {
            id,
            tag,
            props: HashMap::new(),
            text: None,
            parent: None,
            children: Vec::new(),
            yoga,
        }
    }

    pub fn new_text(id: u32, text: String) -> Self {
        let mut node = Self::new(id, InkTag::Text);
        node.text = Some(text);
        node
    }

    pub fn new_spacer(id: u32) -> Self {
        let mut node = Self::new(id, InkTag::Spacer);
        node.yoga.set_flex_grow(1.0);
        node.yoga.set_flex_shrink(1.0);
        node
    }

    pub fn new_newline(id: u32) -> Self {
        let mut node = Self::new(id, InkTag::Newline);
        node.yoga.set_flex_direction(FlexDirection::Column);
        node
    }

    /// Apply props to the Yoga node
    pub fn apply_props(&mut self, props: &HashMap<String, PropValue>) {
        self.props = props.clone();
        apply_flex_props(self, props);
        apply_spacing_props(self, props);
        apply_border_props(self, props);
        apply_dimension_props(self, props);
    }

    /// Get computed layout
    pub fn get_layout(&self) -> yoga::Layout {
        self.yoga.get_layout()
    }
}

/// Apply flex-related props
fn apply_flex_props(node: &mut InkNode, props: &HashMap<String, PropValue>) {
    if let Some(PropValue::String(s)) = props.get("flexDirection") {
        node.yoga.set_flex_direction(match s.as_str() {
            "column" => FlexDirection::Column,
            "column-reverse" => FlexDirection::ColumnReverse,
            "row-reverse" => FlexDirection::RowReverse,
            _ => FlexDirection::Row,
        });
    }

    if let Some(PropValue::String(s)) = props.get("alignItems") {
        node.yoga.set_align_items(match s.as_str() {
            "center" => Align::Center,
            "flex-end" => Align::FlexEnd,
            "stretch" => Align::Stretch,
            "baseline" => Align::Baseline,
            _ => Align::FlexStart,
        });
    }

    // alignSelf - override parent's alignItems for this child
    if let Some(PropValue::String(s)) = props.get("alignSelf") {
        node.yoga.set_align_self(match s.as_str() {
            "center" => Align::Center,
            "flex-end" => Align::FlexEnd,
            "flex-start" => Align::FlexStart,
            "stretch" => Align::Stretch,
            "baseline" => Align::Baseline,
            "auto" => Align::Auto,
            _ => Align::Auto,
        });
    }

    // alignContent - multi-line alignment (for wrapped content)
    if let Some(PropValue::String(s)) = props.get("alignContent") {
        node.yoga.set_align_content(match s.as_str() {
            "center" => Align::Center,
            "flex-end" => Align::FlexEnd,
            "flex-start" => Align::FlexStart,
            "stretch" => Align::Stretch,
            "space-between" => Align::SpaceBetween,
            "space-around" => Align::SpaceAround,
            _ => Align::FlexStart,
        });
    }

    if let Some(PropValue::String(s)) = props.get("justifyContent") {
        node.yoga.set_justify_content(match s.as_str() {
            "center" => Justify::Center,
            "flex-end" => Justify::FlexEnd,
            "space-between" => Justify::SpaceBetween,
            "space-around" => Justify::SpaceAround,
            "space-evenly" => Justify::SpaceEvenly,
            _ => Justify::FlexStart,
        });
    }

    if let Some(PropValue::String(s)) = props.get("flexWrap") {
        node.yoga.set_flex_wrap(match s.as_str() {
            "wrap" => Wrap::Wrap,
            "nowrap" => Wrap::NoWrap,
            _ => Wrap::NoWrap,
        });
    }

    if let Some(PropValue::String(s)) = props.get("display") {
        node.yoga.set_display(match s.as_str() {
            "flex" => Display::Flex,
            "none" => Display::None,
            _ => Display::Flex,
        });
    }

    // Flex grow/shrink (not on Spacer)
    if node.tag != InkTag::Spacer {
        if let Some(PropValue::Number(n)) = props.get("flexGrow") {
            node.yoga.set_flex_grow(*n as f32);
        }
        if let Some(PropValue::Number(n)) = props.get("flexShrink") {
            node.yoga.set_flex_shrink(*n as f32);
        }
    }

    // Flex basis
    if let Some(v) = props.get("flexBasis") {
        match v {
            PropValue::Number(n) => {
                node.yoga.set_flex_basis(StyleUnit::Point(OrderedFloat(*n as f32)));
            }
            PropValue::String(s) if s == "auto" => {
                node.yoga.set_flex_basis(StyleUnit::Auto);
            }
            PropValue::String(s) if s.ends_with('%') => {
                if let Ok(pct) = s.trim_end_matches('%').parse::<f32>() {
                    node.yoga.set_flex_basis(StyleUnit::Percent(OrderedFloat(pct)));
                }
            }
            _ => {}
        }
    }
}

/// Apply spacing (margin, padding, gap) props
fn apply_spacing_props(node: &mut InkNode, props: &HashMap<String, PropValue>) {
    let set_margin = |node: &mut InkNode, edge: yoga::Edge, v: f32| {
        node.yoga.set_margin(edge, StyleUnit::Point(OrderedFloat(v)));
    };
    let set_padding = |node: &mut InkNode, edge: yoga::Edge, v: f32| {
        node.yoga.set_padding(edge, StyleUnit::Point(OrderedFloat(v)));
    };
    let parse_val = |v: &PropValue| parse_spacing_value(v);

    // Margin
    if let Some(v) = props.get("margin").and_then(parse_val) {
        node.yoga.set_margin(yoga::Edge::All, StyleUnit::Point(OrderedFloat(v)));
    }
    if let Some(v) = props.get("marginTop").and_then(parse_val) {
        set_margin(node, yoga::Edge::Top, v);
    }
    if let Some(v) = props.get("marginBottom").and_then(parse_val) {
        set_margin(node, yoga::Edge::Bottom, v);
    }
    if let Some(v) = props.get("marginLeft").and_then(parse_val) {
        set_margin(node, yoga::Edge::Left, v);
    }
    if let Some(v) = props.get("marginRight").and_then(parse_val) {
        set_margin(node, yoga::Edge::Right, v);
    }
    if let Some(v) = props.get("marginY").and_then(parse_val) {
        set_margin(node, yoga::Edge::Top, v);
        set_margin(node, yoga::Edge::Bottom, v);
    }
    if let Some(v) = props.get("marginX").and_then(parse_val) {
        set_margin(node, yoga::Edge::Left, v);
        set_margin(node, yoga::Edge::Right, v);
    }

    // Padding
    if let Some(v) = props.get("padding").and_then(parse_val) {
        node.yoga.set_padding(yoga::Edge::All, StyleUnit::Point(OrderedFloat(v)));
    }
    if let Some(v) = props.get("paddingTop").and_then(parse_val) {
        set_padding(node, yoga::Edge::Top, v);
    }
    if let Some(v) = props.get("paddingBottom").and_then(parse_val) {
        set_padding(node, yoga::Edge::Bottom, v);
    }
    if let Some(v) = props.get("paddingLeft").and_then(parse_val) {
        set_padding(node, yoga::Edge::Left, v);
    }
    if let Some(v) = props.get("paddingRight").and_then(parse_val) {
        set_padding(node, yoga::Edge::Right, v);
    }
    if let Some(v) = props.get("paddingY").and_then(parse_val) {
        set_padding(node, yoga::Edge::Top, v);
        set_padding(node, yoga::Edge::Bottom, v);
    }
    if let Some(v) = props.get("paddingX").and_then(parse_val) {
        set_padding(node, yoga::Edge::Left, v);
        set_padding(node, yoga::Edge::Right, v);
    }

    // Gap (flex gap between children)
    // Supports both Ink 7 names (columnGap/rowGap) and Ink 6 names (gapX/gapY)
    if let Some(PropValue::Number(n)) = props.get("gap") {
        node.yoga.set_gap(yoga::Axis::Horizontal, OrderedFloat(*n as f32));
        node.yoga.set_gap(yoga::Axis::Vertical, OrderedFloat(*n as f32));
    }
    // gapX and columnGap are synonyms
    if let Some(PropValue::Number(n)) = props.get("gapX").or(props.get("columnGap")) {
        node.yoga.set_gap(yoga::Axis::Horizontal, OrderedFloat(*n as f32));
    }
    // gapY and rowGap are synonyms
    if let Some(PropValue::Number(n)) = props.get("gapY").or(props.get("rowGap")) {
        node.yoga.set_gap(yoga::Axis::Vertical, OrderedFloat(*n as f32));
    }
}

/// Apply border props
fn apply_border_props(node: &mut InkNode, props: &HashMap<String, PropValue>) {
    if let Some(PropValue::String(s)) = props.get("borderStyle") {
        let border_width = match s.as_str() {
            "single" | "bold" | "round" | "double" | "singleDouble" | "doubleSingle"
            | "Classic" | "Pascal" | "嘴里" => 1.0,
            "none" => 0.0,
            _ => 0.0,
        };
        if border_width > 0.0 {
            node.yoga.set_border(yoga::Edge::Left, border_width);
            node.yoga.set_border(yoga::Edge::Top, border_width);
            node.yoga.set_border(yoga::Edge::Right, border_width);
            node.yoga.set_border(yoga::Edge::Bottom, border_width);
        }
    }
}

/// Apply dimension props
fn apply_dimension_props(node: &mut InkNode, props: &HashMap<String, PropValue>) {
    // Width
    if let Some(v) = props.get("width") {
        match v {
            PropValue::Number(n) => {
                node.yoga.set_width(StyleUnit::Point(OrderedFloat(*n as f32)));
            }
            PropValue::String(s) if s == "auto" => {
                node.yoga.set_width(StyleUnit::Auto);
            }
            PropValue::String(s) if s.ends_with('%') => {
                if let Ok(pct) = s.trim_end_matches('%').parse::<f32>() {
                    node.yoga.set_width(StyleUnit::Percent(OrderedFloat(pct)));
                }
            }
            _ => {}
        }
    }

    // Height
    if let Some(v) = props.get("height") {
        match v {
            PropValue::Number(n) => {
                node.yoga.set_height(StyleUnit::Point(OrderedFloat(*n as f32)));
            }
            PropValue::String(s) if s == "auto" => {
                node.yoga.set_height(StyleUnit::Auto);
            }
            PropValue::String(s) if s.ends_with('%') => {
                if let Ok(pct) = s.trim_end_matches('%').parse::<f32>() {
                    node.yoga.set_height(StyleUnit::Percent(OrderedFloat(pct)));
                }
            }
            _ => {}
        }
    }

    // Position
    if let Some(PropValue::String(s)) = props.get("position") {
        node.yoga.set_position_type(match s.as_str() {
            "absolute" => PositionType::Absolute,
            _ => PositionType::Relative,
        });
    }

    // Position props (top, right, bottom, left) for absolute positioning
    apply_position_props(node, props);

    // Min/max dimensions
    apply_min_max(node, props);
}

/// Apply min/max dimension constraints
#[allow(clippy::complexity_threshold)]
fn apply_min_max(node: &mut InkNode, props: &HashMap<String, PropValue>) {
    if let Some(PropValue::Number(n)) = props.get("minWidth") {
        node.yoga.set_min_width(StyleUnit::Point(OrderedFloat(*n as f32)));
    }
    if let Some(PropValue::String(s)) = props.get("minWidth") {
        if s.ends_with('%') {
            if let Ok(pct) = s.trim_end_matches('%').parse::<f32>() {
                node.yoga.set_min_width(StyleUnit::Percent(OrderedFloat(pct)));
            }
        }
    }

    if let Some(PropValue::Number(n)) = props.get("maxWidth") {
        node.yoga.set_max_width(StyleUnit::Point(OrderedFloat(*n as f32)));
    }
    if let Some(PropValue::String(s)) = props.get("maxWidth") {
        if s.ends_with('%') {
            if let Ok(pct) = s.trim_end_matches('%').parse::<f32>() {
                node.yoga.set_max_width(StyleUnit::Percent(OrderedFloat(pct)));
            }
        }
    }

    if let Some(PropValue::Number(n)) = props.get("minHeight") {
        node.yoga.set_min_height(StyleUnit::Point(OrderedFloat(*n as f32)));
    }
    if let Some(PropValue::String(s)) = props.get("minHeight") {
        if s.ends_with('%') {
            if let Ok(pct) = s.trim_end_matches('%').parse::<f32>() {
                node.yoga.set_min_height(StyleUnit::Percent(OrderedFloat(pct)));
            }
        }
    }

    if let Some(PropValue::Number(n)) = props.get("maxHeight") {
        node.yoga.set_max_height(StyleUnit::Point(OrderedFloat(*n as f32)));
    }
    if let Some(PropValue::String(s)) = props.get("maxHeight") {
        if s.ends_with('%') {
            if let Ok(pct) = s.trim_end_matches('%').parse::<f32>() {
                node.yoga.set_max_height(StyleUnit::Percent(OrderedFloat(pct)));
            }
        }
    }
}

/// Apply position props (top, right, bottom, left) for absolute positioning
fn apply_position_props(node: &mut InkNode, props: &HashMap<String, PropValue>) {
    let parse_pos = |v: &PropValue| parse_spacing_value(v);

    if let Some(v) = props.get("top").and_then(parse_pos) {
        node.yoga
            .set_position(yoga::Edge::Top, StyleUnit::Point(OrderedFloat(v)));
    }
    if let Some(v) = props.get("right").and_then(parse_pos) {
        node.yoga
            .set_position(yoga::Edge::Right, StyleUnit::Point(OrderedFloat(v)));
    }
    if let Some(v) = props.get("bottom").and_then(parse_pos) {
        node.yoga
            .set_position(yoga::Edge::Bottom, StyleUnit::Point(OrderedFloat(v)));
    }
    if let Some(v) = props.get("left").and_then(parse_pos) {
        node.yoga
            .set_position(yoga::Edge::Left, StyleUnit::Point(OrderedFloat(v)));
    }
}

/// Parse spacing string to f32 value
fn parse_spacing_value(v: &PropValue) -> Option<f32> {
    match v {
        PropValue::Number(n) => Some(*n as f32),
        PropValue::String(s) => s.trim_end_matches("px").trim().parse().ok(),
        _ => None,
    }
}
