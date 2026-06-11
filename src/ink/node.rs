//! Ink node representation
//!
//! Contains InkNode struct with Yoga layout and prop application.

// PropValue is defined in this file, don't re-export
use ordered_float::OrderedFloat;
use std::collections::HashMap;
use unicode_width::UnicodeWidthStr;
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
    /// Cached JSON serialization of props for the render path.
    pub props_json_cache: Option<String>,
    /// Last props JSON bytes for short-circuiting no-op commit_update.
    pub last_props_json: Option<Vec<u8>>,
}

impl InkNode {
    pub fn new(id: u32, tag: InkTag) -> Self {
        let mut yoga = Node::new();
        yoga.set_flex_direction(FlexDirection::Row);
        // CSS default for flex containers is `align-items: stretch`, which
        // makes children fill the cross-axis unless they opt out.  Yoga's
        // default is `flex-start`, which leaves children at their intrinsic
        // size and breaks layouts like `<Box flexGrow={1}>` that rely on
        // auto-stretching to fill the parent.
        yoga.set_align_items(Align::Stretch);
        yoga.set_justify_content(Justify::FlexStart);

        if tag == InkTag::Box {
            yoga.set_align_self(Align::Stretch);
        }

        Self {
            id,
            tag,
            props: HashMap::new(),
            text: None,
            parent: None,
            children: Vec::new(),
            yoga,
            props_json_cache: None,
            last_props_json: None,
        }
    }

    pub fn new_text(id: u32, text: String) -> Self {
        let mut node = Self::new(id, InkTag::Text);
        node.text = Some(text.clone());
        // Set intrinsic dimensions for text so Yoga can lay it out.
        // Use Unicode-aware display width for terminal cells.
        let width = text.width() as f32;
        node.yoga.set_width(StyleUnit::Point(OrderedFloat(width)));
        node.yoga.set_height(StyleUnit::Point(OrderedFloat(1.0)));
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
        self.props_json_cache = Some(props_to_json(props));
        self.last_props_json = Some(self.props_json_cache.as_ref().unwrap().as_bytes().to_vec());
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
    apply_flex_direction(node, props);
    apply_align_props(node, props);
    apply_justify_and_wrap(node, props);
    apply_flex_grow_shrink(node, props);
    apply_flex_basis(node, props);
}

fn apply_flex_direction(node: &mut InkNode, props: &HashMap<String, PropValue>) {
    if let Some(PropValue::String(s)) = props.get("flexDirection") {
        node.yoga.set_flex_direction(match s.as_str() {
            "column" => FlexDirection::Column,
            "column-reverse" => FlexDirection::ColumnReverse,
            "row-reverse" => FlexDirection::RowReverse,
            _ => FlexDirection::Row,
        });
    }
}

fn apply_align_props(node: &mut InkNode, props: &HashMap<String, PropValue>) {
    if let Some(PropValue::String(s)) = props.get("alignItems") {
        node.yoga.set_align_items(parse_align(s));
    }
    if let Some(PropValue::String(s)) = props.get("alignSelf") {
        node.yoga.set_align_self(parse_align_self(s));
    }
    if let Some(PropValue::String(s)) = props.get("alignContent") {
        node.yoga.set_align_content(parse_align_content(s));
    }
}

fn parse_align(s: &str) -> Align {
    match s {
        "center" => Align::Center,
        "flex-end" => Align::FlexEnd,
        "stretch" => Align::Stretch,
        "baseline" => Align::Baseline,
        _ => Align::FlexStart,
    }
}

fn parse_align_self(s: &str) -> Align {
    match s {
        "center" => Align::Center,
        "flex-end" => Align::FlexEnd,
        "flex-start" => Align::FlexStart,
        "stretch" => Align::Stretch,
        "baseline" => Align::Baseline,
        "auto" => Align::Auto,
        _ => Align::Auto,
    }
}

fn parse_align_content(s: &str) -> Align {
    match s {
        "center" => Align::Center,
        "flex-end" => Align::FlexEnd,
        "flex-start" => Align::FlexStart,
        "stretch" => Align::Stretch,
        "space-between" => Align::SpaceBetween,
        "space-around" => Align::SpaceAround,
        _ => Align::FlexStart,
    }
}

fn apply_justify_and_wrap(node: &mut InkNode, props: &HashMap<String, PropValue>) {
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
            _ => Wrap::NoWrap,
        });
    }
    if let Some(PropValue::String(s)) = props.get("display") {
        node.yoga.set_display(match s.as_str() {
            "none" => Display::None,
            _ => Display::Flex,
        });
    }
}

fn apply_flex_grow_shrink(node: &mut InkNode, props: &HashMap<String, PropValue>) {
    if node.tag == InkTag::Spacer {
        return;
    }
    if let Some(PropValue::Number(n)) = props.get("flexGrow") {
        node.yoga.set_flex_grow(*n as f32);
    }
    if let Some(PropValue::Number(n)) = props.get("flexShrink") {
        node.yoga.set_flex_shrink(*n as f32);
    }
}

fn apply_flex_basis(node: &mut InkNode, props: &HashMap<String, PropValue>) {
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
    apply_margin_props(node, props);
    apply_padding_props(node, props);
    apply_gap_props(node, props);
}

fn apply_margin_props(node: &mut InkNode, props: &HashMap<String, PropValue>) {
    if let Some(v) = props.get("margin").and_then(parse_spacing_value) {
        node.yoga.set_margin(yoga::Edge::All, StyleUnit::Point(OrderedFloat(v)));
    }
    for (key, edge) in [
        ("marginTop", yoga::Edge::Top),
        ("marginBottom", yoga::Edge::Bottom),
        ("marginLeft", yoga::Edge::Left),
        ("marginRight", yoga::Edge::Right),
    ] {
        if let Some(v) = props.get(key).and_then(parse_spacing_value) {
            node.yoga.set_margin(edge, StyleUnit::Point(OrderedFloat(v)));
        }
    }
    if let Some(v) = props.get("marginY").and_then(parse_spacing_value) {
        node.yoga.set_margin(yoga::Edge::Top, StyleUnit::Point(OrderedFloat(v)));
        node.yoga.set_margin(yoga::Edge::Bottom, StyleUnit::Point(OrderedFloat(v)));
    }
    if let Some(v) = props.get("marginX").and_then(parse_spacing_value) {
        node.yoga.set_margin(yoga::Edge::Left, StyleUnit::Point(OrderedFloat(v)));
        node.yoga.set_margin(yoga::Edge::Right, StyleUnit::Point(OrderedFloat(v)));
    }
}

fn apply_padding_props(node: &mut InkNode, props: &HashMap<String, PropValue>) {
    if let Some(v) = props.get("padding").and_then(parse_spacing_value) {
        node.yoga.set_padding(yoga::Edge::All, StyleUnit::Point(OrderedFloat(v)));
    }
    for (key, edge) in [
        ("paddingTop", yoga::Edge::Top),
        ("paddingBottom", yoga::Edge::Bottom),
        ("paddingLeft", yoga::Edge::Left),
        ("paddingRight", yoga::Edge::Right),
    ] {
        if let Some(v) = props.get(key).and_then(parse_spacing_value) {
            node.yoga.set_padding(edge, StyleUnit::Point(OrderedFloat(v)));
        }
    }
    if let Some(v) = props.get("paddingY").and_then(parse_spacing_value) {
        node.yoga.set_padding(yoga::Edge::Top, StyleUnit::Point(OrderedFloat(v)));
        node.yoga.set_padding(yoga::Edge::Bottom, StyleUnit::Point(OrderedFloat(v)));
    }
    if let Some(v) = props.get("paddingX").and_then(parse_spacing_value) {
        node.yoga.set_padding(yoga::Edge::Left, StyleUnit::Point(OrderedFloat(v)));
        node.yoga.set_padding(yoga::Edge::Right, StyleUnit::Point(OrderedFloat(v)));
    }
}

fn apply_gap_props(node: &mut InkNode, props: &HashMap<String, PropValue>) {
    if let Some(PropValue::Number(n)) = props.get("gap") {
        let v = yoga::StyleUnit::Point(ordered_float::OrderedFloat(*n as f32));
        node.yoga.set_gap(yoga::Gutter::Column, v);
        node.yoga.set_gap(yoga::Gutter::Row, v);
    }
    if let Some(PropValue::Number(n)) = props.get("gapX").or(props.get("columnGap")) {
        node.yoga.set_gap(yoga::Gutter::Column, yoga::StyleUnit::Point(ordered_float::OrderedFloat(*n as f32)));
    }
    if let Some(PropValue::Number(n)) = props.get("gapY").or(props.get("rowGap")) {
        node.yoga.set_gap(yoga::Gutter::Row, yoga::StyleUnit::Point(ordered_float::OrderedFloat(*n as f32)));
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

/// Serialize props HashMap to JSON string.
pub fn props_to_json(props: &HashMap<String, PropValue>) -> String {
    let mut map = serde_json::Map::new();
    for (k, v) in props {
        map.insert(k.clone(), prop_value_to_json_value(v));
    }
    serde_json::Value::Object(map).to_string()
}

fn prop_value_to_json_value(v: &PropValue) -> serde_json::Value {
    match v {
        PropValue::Null => serde_json::Value::Null,
        PropValue::Bool(b) => serde_json::Value::Bool(*b),
        PropValue::Number(n) => serde_json::Value::Number(
            serde_json::Number::from_f64(*n).unwrap_or_else(|| serde_json::Number::from(0)),
        ),
        PropValue::String(s) => serde_json::Value::String(s.clone()),
        PropValue::Vec(arr) => {
            serde_json::Value::Array(arr.iter().map(prop_value_to_json_value).collect())
        }
    }
}

/// Apply dimension props
fn apply_dimension_props(node: &mut InkNode, props: &HashMap<String, PropValue>) {
    let has_width = apply_width(node, props);
    let has_height = apply_height(node, props);

    if node.tag == InkTag::Box && (has_width || has_height) {
        node.yoga.set_align_self(Align::Auto);
    }

    if let Some(PropValue::String(s)) = props.get("position") {
        node.yoga.set_position_type(match s.as_str() {
            "absolute" => PositionType::Absolute,
            _ => PositionType::Relative,
        });
    }

    apply_position_props(node, props);
    apply_min_max(node, props);
}

fn apply_width(node: &mut InkNode, props: &HashMap<String, PropValue>) -> bool {
    if let Some(v) = props.get("width") {
        match v {
            PropValue::Number(n) => node.yoga.set_width(StyleUnit::Point(OrderedFloat(*n as f32))),
            PropValue::String(s) if s == "auto" => node.yoga.set_width(StyleUnit::Auto),
            PropValue::String(s) if s.ends_with('%') => {
                if let Ok(pct) = s.trim_end_matches('%').parse::<f32>() {
                    node.yoga.set_width(StyleUnit::Percent(OrderedFloat(pct)));
                }
            }
            _ => {}
        }
        return true;
    }
    false
}

fn apply_height(node: &mut InkNode, props: &HashMap<String, PropValue>) -> bool {
    if let Some(v) = props.get("height") {
        match v {
            PropValue::Number(n) => node.yoga.set_height(StyleUnit::Point(OrderedFloat(*n as f32))),
            PropValue::String(s) if s == "auto" => node.yoga.set_height(StyleUnit::Auto),
            PropValue::String(s) if s.ends_with('%') => {
                if let Ok(pct) = s.trim_end_matches('%').parse::<f32>() {
                    node.yoga.set_height(StyleUnit::Percent(OrderedFloat(pct)));
                }
            }
            _ => {}
        }
        return true;
    }
    false
}

/// Apply min/max dimension constraints
fn apply_min_max(node: &mut InkNode, props: &HashMap<String, PropValue>) {
    apply_min_max_prop(node, props, "minWidth",  |n, v| n.yoga.set_min_width(v));
    apply_min_max_prop(node, props, "maxWidth",  |n, v| n.yoga.set_max_width(v));
    apply_min_max_prop(node, props, "minHeight", |n, v| n.yoga.set_min_height(v));
    apply_min_max_prop(node, props, "maxHeight", |n, v| n.yoga.set_max_height(v));
}

fn apply_min_max_prop(
    node: &mut InkNode,
    props: &HashMap<String, PropValue>,
    key: &str,
    mut setter: impl FnMut(&mut InkNode, StyleUnit),
) {
    if let Some(v) = props.get(key) {
        match v {
            PropValue::Number(n) => {
                setter(node, StyleUnit::Point(OrderedFloat(*n as f32)));
            }
            PropValue::String(s) if s.ends_with('%') => {
                if let Ok(pct) = s.trim_end_matches('%').parse::<f32>() {
                    setter(node, StyleUnit::Percent(OrderedFloat(pct)));
                }
            }
            _ => {}
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
