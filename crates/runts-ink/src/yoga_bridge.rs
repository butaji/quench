//! Bridge between runts-ink's `Box` type and
//! the Yoga flexbox layout engine.
//!
//! Yoga is the same layout engine that real Ink
//! uses (Ink depends on the `yoga-layout` npm
//! package, which wraps the C++ Yoga
//! implementation from Meta/Facebook). The Rust
//! `yoga` crate (https://github.com/bschwind/yoga-rs)
//! compiles the same C++ Yoga from source via
//! bindgen, so we get byte-for-byte identical
//! layout results to Ink.
//!
//! This module converts a `Box` into a
//! `Vec<FlexStyle>` (a list of style properties
//! that can be applied to a Yoga node with
//! `Node::apply_styles(&styles)`).

use crate::components::{
    AlignContent, AlignItems, AlignSelf, Box, Display, FlexDirection, FlexWrap, JustifyContent,
    Overflow, Position,
};
use crate::style::Wrap as InkWrap;

/// Convert a `Box` to a vector of Yoga
/// `FlexStyle` variants. The returned vec can
/// be applied to a `yoga::Node` with
/// `node.apply_styles(&styles)`.
pub fn style_for_box(b: &Box) -> Vec<yoga::FlexStyle> {
    use yoga::FlexStyle::*;
    use yoga::prelude::*;

    let mut styles: Vec<FlexStyle> = Vec::new();

    // Flex direction
    let dir = match b.flex_direction {
        FlexDirection::Row => yoga::FlexDirection::Row,
        FlexDirection::Column => yoga::FlexDirection::Column,
        FlexDirection::RowReverse => yoga::FlexDirection::RowReverse,
        FlexDirection::ColumnReverse => yoga::FlexDirection::ColumnReverse,
    };
    styles.push(FlexDirection(dir));

    // Flex wrap
    let wrap = match b.flex_wrap {
        FlexWrap::NoWrap => yoga::Wrap::NoWrap,
        FlexWrap::Wrap => yoga::Wrap::Wrap,
        FlexWrap::WrapReverse => yoga::Wrap::WrapReverse,
    };
    styles.push(FlexWrap(wrap));

    // Justify content
    let justify = match b.justify_content {
        JustifyContent::FlexStart => yoga::Justify::FlexStart,
        JustifyContent::FlexEnd => yoga::Justify::FlexEnd,
        JustifyContent::Center => yoga::Justify::Center,
        JustifyContent::SpaceBetween => yoga::Justify::SpaceBetween,
        JustifyContent::SpaceAround => yoga::Justify::SpaceAround,
        JustifyContent::SpaceEvenly => yoga::Justify::SpaceEvenly,
    };
    styles.push(JustifyContent(justify));

    // Align items
    let align_items = match b.align_items {
        AlignItems::FlexStart => yoga::Align::FlexStart,
        AlignItems::FlexEnd => yoga::Align::FlexEnd,
        AlignItems::Center => yoga::Align::Center,
        AlignItems::Stretch => yoga::Align::Stretch,
        AlignItems::Baseline => yoga::Align::Baseline,
    };
    styles.push(AlignItems(align_items));

    // Align self
    let align_self = match b.align_self {
        AlignSelf::Auto => yoga::Align::Auto,
        AlignSelf::FlexStart => yoga::Align::FlexStart,
        AlignSelf::FlexEnd => yoga::Align::FlexEnd,
        AlignSelf::Center => yoga::Align::Center,
        AlignSelf::Stretch => yoga::Align::Stretch,
        AlignSelf::Baseline => yoga::Align::Baseline,
    };
    styles.push(AlignSelf(align_self));

    // Align content
    let align_content = match b.align_content {
        AlignContent::FlexStart => yoga::Align::FlexStart,
        AlignContent::FlexEnd => yoga::Align::FlexEnd,
        AlignContent::Center => yoga::Align::Center,
        AlignContent::Stretch => yoga::Align::Stretch,
        AlignContent::SpaceBetween => yoga::Align::SpaceBetween,
        AlignContent::SpaceAround => yoga::Align::SpaceAround,
    };
    styles.push(AlignContent(align_content));

    // Flex grow / shrink / basis
    styles.push(FlexGrow(b.flex_grow.into()));
    styles.push(FlexShrink(b.flex_shrink.into()));
    let basis = if b.flex_basis_pct > 0.0 {
        yoga::StyleUnit::Percent(b.flex_basis_pct as f32)
    } else {
        yoga::StyleUnit::Auto
    };
    styles.push(FlexBasis(basis));

    // Width / height
    styles.push(Width(opt_to_unit(b.width)));
    styles.push(Height(opt_to_unit(b.height)));
    styles.push(MinWidth(opt_to_unit(b.min_width)));
    styles.push(MinHeight(opt_to_unit(b.min_height)));
    styles.push(MaxWidth(opt_to_unit(b.max_width)));
    styles.push(MaxHeight(opt_to_unit(b.max_height)));

    // Margins
    styles.push(MarginLeft(opt_to_unit(b.margin_left)));
    styles.push(MarginRight(opt_to_unit(b.margin_right)));
    styles.push(MarginTop(opt_to_unit(b.margin_top)));
    styles.push(MarginBottom(opt_to_unit(b.margin_bottom)));

    // Padding
    styles.push(PaddingLeft(opt_to_unit(b.padding_left)));
    styles.push(PaddingRight(opt_to_unit(b.padding_right)));
    styles.push(PaddingTop(opt_to_unit(b.padding_top)));
    styles.push(PaddingBottom(opt_to_unit(b.padding_bottom)));

    // Borders
    styles.push(BorderLeft(if b.borders.left { 1.0 } else { 0.0 }.into()));
    styles.push(BorderRight(if b.borders.right { 1.0 } else { 0.0 }.into()));
    styles.push(BorderTop(if b.borders.top { 1.0 } else { 0.0 }.into()));
    styles.push(BorderBottom(if b.borders.bottom { 1.0 } else { 0.0 }.into()));

    // Gaps
    if let Some(g) = b.row_gap {
        styles.push(RowGap(yoga::StyleUnit::Point(g as f32)));
    }
    if let Some(g) = b.column_gap {
        styles.push(ColumnGap(yoga::StyleUnit::Point(g as f32)));
    }

    // Position
    if matches!(b.position, Position::Absolute) {
        styles.push(PositionType(yoga::PositionType::Absolute));
        styles.push(Position(yoga::Edge::Top, opt_to_unit(b.top)));
        styles.push(Position(yoga::Edge::Right, opt_to_unit(b.right)));
        styles.push(Position(yoga::Edge::Bottom, opt_to_unit(b.bottom)));
        styles.push(Position(yoga::Edge::Left, opt_to_unit(b.left)));
    }

    // Display
    if matches!(b.display, Display::None) {
        styles.push(Display(yoga::Display::None));
    }

    // Overflow
    if matches!(b.overflow_x, Overflow::Hidden) || matches!(b.overflow_y, Overflow::Hidden) {
        styles.push(Overflow(yoga::Overflow::Hidden));
    }

    // Aspect ratio — not stored on Box yet, skip

    styles
}

fn opt_to_unit(v: Option<u16>) -> yoga::StyleUnit {
    match v {
        Some(n) => yoga::StyleUnit::Point(n as f32),
        None => yoga::StyleUnit::Auto,
    }
}

/// Convert a `Box` to a `yoga::Node` (a leaf
/// node with the box's style applied). Children
/// are added separately via `insert_child`.
pub fn new_yoga_node(b: &Box) -> yoga::Node {
    let mut node = yoga::Node::new();
    let styles = style_for_box(b);
    node.apply_styles(&styles);
    node
}
