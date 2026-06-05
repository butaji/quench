//! Taffy-based flexbox layout engine for `runts-ink`.
//!
//! This module bridges the VNode tree to the Taffy layout
//! library.  It is selected by the `taffy` Cargo feature (the
//! default for `runts-ink`).

use taffy::geometry::{Point, Rect as TaffyRect, Size};
use taffy::style::{
    AlignContent as TaffyAlignContent, AlignItems as TaffyAlignItems,
    AvailableSpace, Dimension, Display as TaffyDisplay,
    FlexDirection as TaffyFlexDirection, FlexWrap as TaffyFlexWrap, JustifyContent as TaffyJustifyContent,
    LengthPercentage, LengthPercentageAuto, Overflow as TaffyOverflow, Position as TaffyPosition, Style,
};
use taffy::style_helpers::TaffyZero;
use taffy::tree::{Layout as TaffyLayout, NodeId, TaffyTree};
use unicode_width::UnicodeWidthStr;

use crate::components::{
    AlignContent as InkAlignContent, AlignItems as InkAlignItems, AlignSelf as InkAlignSelf,
    Box as InkBox, FlexDirection as InkFlexDirection, FlexWrap as InkFlexWrap,
    JustifyContent as InkJustifyContent, Text,
};
use crate::style::{Display as InkDisplay, Overflow as InkOverflow, Position as InkPosition};
use crate::vnode::{VNode, VNodeContent};

/// A computed rectangle: x, y, width, height in terminal cells.
pub type Rect = (u16, u16, u16, u16);

/// Layout result: per-VNode rects indexed by DFS pre-order position.
pub struct Layout {
    /// Rect for each VNode in DFS order.
    pub rects: Vec<Rect>,
}

struct LayoutEntry {
    vnode: VNode,
    node_id: Option<NodeId>,
}

/// Compute the layout for a VNode tree within the given viewport.
pub fn compute(root: &VNode, viewport_w: u16, viewport_h: u16) -> Layout {
    let mut taffy = TaffyTree::new();
    let mut entries: Vec<LayoutEntry> = Vec::new();
    build_node(root, &mut taffy, &mut entries, None);

    if let Some(root_id) = entries.first().and_then(|e| e.node_id) {
        let _ = taffy.compute_layout(
            root_id,
            Size {
                width: AvailableSpace::Definite(viewport_w as f32),
                height: AvailableSpace::Definite(viewport_h as f32),
            },
        );
    }

    let mut rects = Vec::with_capacity(entries.len());
    let mut cursor = 0usize;
    collect_rects(&entries, &taffy, &mut cursor, 0.0, 0.0, &mut rects);

    Layout { rects }
}

/* -------------------------------------------------------------------------- */
/* Tree construction                                                          */
/* -------------------------------------------------------------------------- */

fn build_node(
    vnode: &VNode,
    taffy: &mut TaffyTree,
    entries: &mut Vec<LayoutEntry>,
    parent: Option<NodeId>,
) {
    if let VNodeContent::Fragment(fs) = &vnode.0 {
        entries.push(LayoutEntry {
            vnode: vnode.clone(),
            node_id: None,
        });
        for child in fs {
            build_node(child, taffy, entries, parent);
        }
        return;
    }

    let style = vnode_style(vnode);
    let node_id = taffy.new_leaf(style).expect("taffy new_leaf");

    entries.push(LayoutEntry {
        vnode: vnode.clone(),
        node_id: Some(node_id),
    });

    if let Some(p) = parent {
        taffy.add_child(p, node_id).expect("taffy add_child");
    }

    if is_display_none(vnode) {
        return;
    }

    for child in children_of(vnode) {
        build_node(child, taffy, entries, Some(node_id));
    }
}

fn is_display_none(vnode: &VNode) -> bool {
    matches!(
        &vnode.0,
        VNodeContent::Box(InkBox { display: InkDisplay::None, .. })
    )
}

fn children_of(vnode: &VNode) -> &[VNode] {
    match &vnode.0 {
        VNodeContent::Box(b) => &b.children,
        VNodeContent::Static(s) => &s.children,
        VNodeContent::Transform(t) => std::slice::from_ref(&t.child),
        VNodeContent::Fragment(_)
        | VNodeContent::Text(_)
        | VNodeContent::Newline(_)
        | VNodeContent::Spacer(_) => &[],
    }
}

/* -------------------------------------------------------------------------- */
/* Style mapping                                                              */
/* -------------------------------------------------------------------------- */

fn vnode_style(vnode: &VNode) -> Style {
    match &vnode.0 {
        VNodeContent::Box(b) => box_style(b),
        VNodeContent::Text(t) => text_style(t),
        VNodeContent::Newline(_) => newline_style(),
        VNodeContent::Spacer(_) => spacer_style(),
        VNodeContent::Static(_) => static_style(),
        VNodeContent::Transform(_) => Style::default(),
        VNodeContent::Fragment(_) => unreachable!("Fragments do not get Taffy nodes"),
    }
}

fn box_style(b: &InkBox) -> Style {
    Style {
        display: match b.display {
            InkDisplay::Flex => TaffyDisplay::Flex,
            InkDisplay::None => TaffyDisplay::None,
        },
        flex_direction: map_flex_direction(b.flex_direction),
        flex_wrap: map_flex_wrap(b.flex_wrap),
        flex_grow: b.flex_grow,
        flex_shrink: b.flex_shrink,
        flex_basis: if b.flex_basis_pct > 0.0 {
            Dimension::percent(b.flex_basis_pct)
        } else {
            Dimension::auto()
        },
        size: Size {
            width: opt_dim(b.width),
            height: opt_dim(b.height),
        },
        min_size: Size {
            width: opt_dim(b.min_width),
            height: opt_dim(b.min_height),
        },
        max_size: Size {
            width: opt_dim(b.max_width),
            height: opt_dim(b.max_height),
        },
        margin: TaffyRect {
            left: opt_lpauto(b.margin_left),
            right: opt_lpauto(b.margin_right),
            top: opt_lpauto(b.margin_top),
            bottom: opt_lpauto(b.margin_bottom),
        },
        padding: TaffyRect {
            left: opt_lp(b.padding_left),
            right: opt_lp(b.padding_right),
            top: opt_lp(b.padding_top),
            bottom: opt_lp(b.padding_bottom),
        },
        border: TaffyRect {
            left: if b.borders.left {
                LengthPercentage::length(1.0)
            } else {
                LengthPercentage::ZERO
            },
            right: if b.borders.right {
                LengthPercentage::length(1.0)
            } else {
                LengthPercentage::ZERO
            },
            top: if b.borders.top {
                LengthPercentage::length(1.0)
            } else {
                LengthPercentage::ZERO
            },
            bottom: if b.borders.bottom {
                LengthPercentage::length(1.0)
            } else {
                LengthPercentage::ZERO
            },
        },
        align_items: Some(map_align_items(b.align_items)),
        align_self: map_align_self(b.align_self),
        align_content: Some(map_align_content(b.align_content)),
        justify_content: Some(map_justify_content(b.justify_content)),
        position: map_position(b.position),
        inset: TaffyRect {
            left: opt_lpauto(b.left),
            right: opt_lpauto(b.right),
            top: opt_lpauto(b.top),
            bottom: opt_lpauto(b.bottom),
        },
        overflow: Point {
            x: map_overflow(b.overflow_x),
            y: map_overflow(b.overflow_y),
        },
        gap: Size {
            width: opt_lp(b.column_gap),
            height: opt_lp(b.row_gap),
        },
        ..Style::default()
    }
}

fn text_style(t: &Text) -> Style {
    let width = t.content.width() as f32;
    let height = if t.content.is_empty() { 0.0 } else { 1.0 };
    Style {
        size: Size {
            width: Dimension::length(width),
            height: Dimension::length(height),
        },
        ..Style::default()
    }
}

fn newline_style() -> Style {
    Style {
        size: Size {
            width: Dimension::length(0.0),
            height: Dimension::length(1.0),
        },
        ..Style::default()
    }
}

fn spacer_style() -> Style {
    Style {
        flex_grow: 1.0,
        size: Size {
            width: Dimension::auto(),
            height: Dimension::auto(),
        },
        ..Style::default()
    }
}

fn static_style() -> Style {
    Style {
        flex_direction: TaffyFlexDirection::Column,
        size: Size {
            width: Dimension::auto(),
            height: Dimension::auto(),
        },
        ..Style::default()
    }
}

fn opt_dim(v: Option<u16>) -> Dimension {
    match v {
        Some(n) => Dimension::length(n as f32),
        None => Dimension::auto(),
    }
}

fn opt_lp(v: Option<u16>) -> LengthPercentage {
    match v {
        Some(n) => LengthPercentage::length(n as f32),
        None => LengthPercentage::ZERO,
    }
}

fn opt_lpauto(v: Option<u16>) -> LengthPercentageAuto {
    match v {
        Some(n) => LengthPercentageAuto::length(n as f32),
        None => LengthPercentageAuto::ZERO,
    }
}

fn map_flex_direction(d: InkFlexDirection) -> TaffyFlexDirection {
    match d {
        InkFlexDirection::Row => TaffyFlexDirection::Row,
        InkFlexDirection::Column => TaffyFlexDirection::Column,
        InkFlexDirection::RowReverse => TaffyFlexDirection::RowReverse,
        InkFlexDirection::ColumnReverse => TaffyFlexDirection::ColumnReverse,
    }
}

fn map_flex_wrap(w: InkFlexWrap) -> TaffyFlexWrap {
    match w {
        InkFlexWrap::NoWrap => TaffyFlexWrap::NoWrap,
        InkFlexWrap::Wrap => TaffyFlexWrap::Wrap,
        InkFlexWrap::WrapReverse => TaffyFlexWrap::WrapReverse,
    }
}

fn map_align_items(a: InkAlignItems) -> TaffyAlignItems {
    match a {
        InkAlignItems::FlexStart => TaffyAlignItems::FlexStart,
        InkAlignItems::Center => TaffyAlignItems::Center,
        InkAlignItems::FlexEnd => TaffyAlignItems::FlexEnd,
        InkAlignItems::Stretch => TaffyAlignItems::Stretch,
        InkAlignItems::Baseline => TaffyAlignItems::Baseline,
    }
}

fn map_align_self(a: InkAlignSelf) -> Option<TaffyAlignItems> {
    match a {
        InkAlignSelf::Auto => None,
        InkAlignSelf::FlexStart => Some(TaffyAlignItems::FlexStart),
        InkAlignSelf::Center => Some(TaffyAlignItems::Center),
        InkAlignSelf::FlexEnd => Some(TaffyAlignItems::FlexEnd),
        InkAlignSelf::Stretch => Some(TaffyAlignItems::Stretch),
        InkAlignSelf::Baseline => Some(TaffyAlignItems::Baseline),
    }
}

fn map_align_content(a: InkAlignContent) -> TaffyAlignContent {
    match a {
        InkAlignContent::FlexStart => TaffyAlignContent::FlexStart,
        InkAlignContent::Center => TaffyAlignContent::Center,
        InkAlignContent::FlexEnd => TaffyAlignContent::FlexEnd,
        InkAlignContent::Stretch => TaffyAlignContent::Stretch,
        InkAlignContent::SpaceBetween => TaffyAlignContent::SpaceBetween,
        InkAlignContent::SpaceAround => TaffyAlignContent::SpaceAround,
    }
}

fn map_justify_content(j: InkJustifyContent) -> TaffyJustifyContent {
    match j {
        InkJustifyContent::FlexStart => TaffyJustifyContent::FlexStart,
        InkJustifyContent::Center => TaffyJustifyContent::Center,
        InkJustifyContent::FlexEnd => TaffyJustifyContent::FlexEnd,
        InkJustifyContent::SpaceBetween => TaffyJustifyContent::SpaceBetween,
        InkJustifyContent::SpaceAround => TaffyJustifyContent::SpaceAround,
        InkJustifyContent::SpaceEvenly => TaffyJustifyContent::SpaceEvenly,
    }
}

fn map_position(p: InkPosition) -> TaffyPosition {
    match p {
        InkPosition::Relative => TaffyPosition::Relative,
        InkPosition::Absolute => TaffyPosition::Absolute,
    }
}

fn map_overflow(o: InkOverflow) -> TaffyOverflow {
    match o {
        InkOverflow::Visible => TaffyOverflow::Visible,
        InkOverflow::Hidden => TaffyOverflow::Hidden,
    }
}

/* -------------------------------------------------------------------------- */
/* Rect collection                                                            */
/* -------------------------------------------------------------------------- */

fn collect_rects(
    entries: &[LayoutEntry],
    taffy: &TaffyTree,
    cursor: &mut usize,
    parent_x: f32,
    parent_y: f32,
    rects: &mut Vec<Rect>,
) {
    if *cursor >= entries.len() {
        return;
    }
    let entry = &entries[*cursor];
    *cursor += 1;

    match entry.node_id {
        Some(id) => {
            let layout = taffy.layout(id).expect("taffy layout missing");
            let x = parent_x + layout.location.x;
            let y = parent_y + layout.location.y;
            let w = layout.size.width;
            let h = layout.size.height;
            rects.push((x as u16, y as u16, w as u16, h as u16));

            match &entry.vnode.0 {
                VNodeContent::Box(b) if !matches!(b.display, InkDisplay::None) => {
                    for _ in 0..b.children.len() {
                        collect_rects(entries, taffy, cursor, x, y, rects);
                    }
                }
                VNodeContent::Static(s) => {
                    for _ in 0..s.children.len() {
                        collect_rects(entries, taffy, cursor, x, y, rects);
                    }
                }
                VNodeContent::Transform(t) => {
                    collect_rects(entries, taffy, cursor, x, y, rects);
                    if let Some(last) = rects.last_mut() {
                        let nx = (last.0 as i32 + t.x as i32).max(0) as u16;
                        let ny = (last.1 as i32 + t.y as i32).max(0) as u16;
                        *last = (nx, ny, last.2, last.3);
                    }
                }
                _ => {}
            }
        }
        None => {
            rects.push((0, 0, 0, 0));
            if let VNodeContent::Fragment(fs) = &entry.vnode.0 {
                for _ in 0..fs.len() {
                    collect_rects(entries, taffy, cursor, parent_x, parent_y, rects);
                }
            }
        }
    }
}

/* -------------------------------------------------------------------------- */
/* Public compatibility stubs                                                 */
/* -------------------------------------------------------------------------- */

/// Stub kept for API compatibility with the old layout bridge.
pub fn style_for_box(_b: &InkBox) {}

/// Stub kept for API compatibility with the old layout bridge.
pub fn style_for_text() {}

/// Stub kept for API compatibility with the old layout bridge.
pub fn style_for_spacer(_g: f32) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Box as InkBox;

    #[test]
    fn taffy_layout_text_in_box() {
        let root = InkBox::column()
            .padding(1)
            .child(VNode::from(Text::new("Hello")))
            .into();
        let layout = compute(&root, 40, 10);
        assert!(!layout.rects.is_empty());
        let (_x, _y, w, h) = layout.rects[0];
        assert!(w > 0 && h > 0);
    }

    #[test]
    fn taffy_row_layout_positions_children_side_by_side() {
        let root = InkBox::row()
            .child(VNode::from(Text::new("A")))
            .child(VNode::from(Text::new("B")))
            .into();
        let layout = compute(&root, 40, 4);
        assert!(layout.rects.len() >= 3);
        let (_, _, aw, _) = layout.rects[1];
        let (bx, _, _, _) = layout.rects[2];
        assert_eq!(aw, 1);
        assert_eq!(bx, aw);
    }

    #[test]
    fn taffy_margin_box_size() {
        use crate::components::Box as InkBox;
        let mut left_box = InkBox::new();
        left_box.margin_right = Some(2);
        left_box.border_style = crate::style::BorderStyle::Single;
        left_box.borders = crate::style::Borders::ALL;
        left_box.padding_top = Some(1);
        left_box.padding_bottom = Some(1);
        left_box.padding_left = Some(1);
        left_box.padding_right = Some(1);

        left_box = left_box.child(VNode::from(Text::new("Left")));
        let row = InkBox::row()
            .width(80)
            .child(VNode::from(left_box));
        let root: VNode = row.into();
        let layout = compute(&root, 80, 24);
        eprintln!("margin box rects (explicit row width): {:?}", layout.rects);
        assert!(layout.rects.len() >= 3);
        let (_, _, bw, bh) = layout.rects[1];
        assert_eq!((bw, bh), (8, 5), "margin box should be 8x5, got {}x{}", bw, bh);
        let (_, _, tw, th) = layout.rects[2];
        assert_eq!((tw, th), (4, 1), "text should be 4x1, got {}x{}", tw, th);
    }

    #[test]
    fn taffy_margin_box_in_auto_row() {
        use crate::components::Box as InkBox;
        let mut left_box = InkBox::new();
        left_box.margin_right = Some(2);
        left_box.border_style = crate::style::BorderStyle::Single;
        left_box.borders = crate::style::Borders::ALL;
        left_box.padding_top = Some(1);
        left_box.padding_bottom = Some(1);
        left_box.padding_left = Some(1);
        left_box.padding_right = Some(1);
        left_box = left_box.child(VNode::from(Text::new("Left")));

        let mut right_box = InkBox::new();
        right_box.border_style = crate::style::BorderStyle::Single;
        right_box.borders = crate::style::Borders::ALL;
        right_box.padding_top = Some(1);
        right_box.padding_bottom = Some(1);
        right_box.padding_left = Some(1);
        right_box.padding_right = Some(1);
        right_box = right_box.child(VNode::from(Text::new("Right")));

        let row = InkBox::row()
            .child(VNode::from(left_box))
            .child(VNode::from(right_box));

        let root = InkBox::column()
            .padding(1)
            .child(row);
        let layout = compute(&root.into(), 80, 24);
        eprintln!("margin box rects (auto row width): {:?}", layout.rects);
        assert!(layout.rects.len() >= 4);
        let (_, _, bw, bh) = layout.rects[2];
        assert_eq!((bw, bh), (8, 5), "margin box should be 8x5, got {}x{}", bw, bh);
    }

    #[test]
    fn taffy_layout_honours_display_none() {
        let mut hidden = InkBox::column();
        hidden.display = InkDisplay::None;
        hidden = hidden.child(VNode::from(Text::new("secret")));
        let root = InkBox::column().child(VNode::from(hidden)).into();
        let layout = compute(&root, 40, 4);
        assert_eq!(layout.rects.len(), 2);
        let (_, _, w, h) = layout.rects[1];
        assert_eq!((w, h), (0, 0));
    }
}
