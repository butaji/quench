//! Yoga-based flexbox layout engine for `runts-ink`.
//!
//! This module bridges the VNode tree to the Yoga layout
//! library (the same engine Ink uses internally).  It is
//! selected by the `yoga` Cargo feature.

use ordered_float::OrderedFloat;
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
    node_idx: Option<usize>,
}

/// Compute the layout for a VNode tree within the given viewport.
pub fn compute(root: &VNode, viewport_w: u16, viewport_h: u16) -> Layout {
    let mut entries: Vec<LayoutEntry> = Vec::new();
    let mut nodes: Vec<yoga::Node> = Vec::new();

    build_node(root, &mut entries, &mut nodes, None);

    if let Some(root_idx) = entries.iter().enumerate().find_map(|(i, e)| e.node_idx.filter(|_| i == 0)) {
        // Ink only fixes the root width to the viewport; height is
        // left indefinite so the tree can grow to fit content.
        nodes[root_idx].apply_styles(&[
            yoga::FlexStyle::Width(pt(viewport_w)),
        ]);
        nodes[root_idx].calculate_layout(viewport_w as f32, f32::NAN, yoga::Direction::LTR);
    }

    let mut rects = Vec::with_capacity(entries.len());
    let mut cursor = 0usize;
    compute_absolute(&entries, &nodes, &mut cursor, 0.0, 0.0, &mut rects);

    Layout { rects }
}

/* -------------------------------------------------------------------------- */
/* Tree building                                                              */
/* -------------------------------------------------------------------------- */

fn build_node(
    vnode: &VNode,
    entries: &mut Vec<LayoutEntry>,
    nodes: &mut Vec<yoga::Node>,
    parent_idx: Option<usize>,
) {
    if let VNodeContent::Fragment(fs) = &vnode.0 {
        entries.push(LayoutEntry {
            vnode: vnode.clone(),
            node_idx: None,
        });
        for child in fs {
            build_node(child, entries, nodes, parent_idx);
        }
        return;
    }

    let mut node = yoga::Node::new();
    apply_vnode_style(&mut node, vnode);

    let idx = nodes.len();
    nodes.push(node);
    entries.push(LayoutEntry {
        vnode: vnode.clone(),
        node_idx: Some(idx),
    });

    if let Some(p) = parent_idx {
        let (before, after) = nodes.split_at_mut(idx);
        let parent = before.get_mut(p).expect("parent node missing");
        let child = after.get_mut(0).expect("child node missing");
        let insert_at = parent.get_child_count();
        parent.insert_child(child, insert_at);
    }

    if is_display_none(vnode) {
        return;
    }

    for child in children_of(vnode) {
        build_node(child, entries, nodes, Some(idx));
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
        VNodeContent::Fragment(_) | VNodeContent::Text(_) | VNodeContent::Newline(_) | VNodeContent::Spacer(_) => &[],
    }
}

/* -------------------------------------------------------------------------- */
/* Style mapping                                                              */
/* -------------------------------------------------------------------------- */

fn apply_vnode_style(node: &mut yoga::Node, vnode: &VNode) {
    match &vnode.0 {
        VNodeContent::Box(b) => apply_box_style(node, b),
        VNodeContent::Text(t) => apply_text_style(node, t),
        VNodeContent::Newline(_) => apply_newline_style(node),
        VNodeContent::Spacer(_) => apply_spacer_style(node),
        VNodeContent::Static(_) => apply_static_style(node),
        VNodeContent::Transform(_) => { /* transparent container */ }
        VNodeContent::Fragment(_) => unreachable!("Fragments do not get Yoga nodes"),
    }
}

// allow:too_many_lines
fn apply_box_style(node: &mut yoga::Node, b: &InkBox) {
    use yoga::FlexStyle;

    let mut styles: Vec<FlexStyle> = Vec::new();

    styles.push(FlexStyle::Display(match b.display {
        InkDisplay::Flex => yoga::Display::Flex,
        InkDisplay::None => yoga::Display::None,
    }));

    styles.push(FlexStyle::FlexDirection(match b.flex_direction {
        InkFlexDirection::Row => yoga::FlexDirection::Row,
        InkFlexDirection::Column => yoga::FlexDirection::Column,
        InkFlexDirection::RowReverse => yoga::FlexDirection::RowReverse,
        InkFlexDirection::ColumnReverse => yoga::FlexDirection::ColumnReverse,
    }));

    styles.push(FlexStyle::FlexWrap(match b.flex_wrap {
        InkFlexWrap::NoWrap => yoga::Wrap::NoWrap,
        InkFlexWrap::Wrap => yoga::Wrap::Wrap,
        InkFlexWrap::WrapReverse => yoga::Wrap::WrapReverse,
    }));

    styles.push(FlexStyle::FlexGrow(OrderedFloat(b.flex_grow)));
    styles.push(FlexStyle::FlexShrink(OrderedFloat(b.flex_shrink)));

    if b.flex_basis_pct > 0.0 {
        styles.push(FlexStyle::FlexBasis(pct(b.flex_basis_pct)));
    } else {
        styles.push(FlexStyle::FlexBasis(auto()));
    }

    styles.push(FlexStyle::Width(opt_pt(b.width)));
    styles.push(FlexStyle::Height(opt_pt(b.height)));
    styles.push(FlexStyle::MinWidth(opt_pt(b.min_width)));
    styles.push(FlexStyle::MinHeight(opt_pt(b.min_height)));
    styles.push(FlexStyle::MaxWidth(opt_pt(b.max_width)));
    styles.push(FlexStyle::MaxHeight(opt_pt(b.max_height)));

    if let Some(v) = b.padding_top {
        styles.push(FlexStyle::PaddingTop(pt(v)));
    }
    if let Some(v) = b.padding_right {
        styles.push(FlexStyle::PaddingRight(pt(v)));
    }
    if let Some(v) = b.padding_bottom {
        styles.push(FlexStyle::PaddingBottom(pt(v)));
    }
    if let Some(v) = b.padding_left {
        styles.push(FlexStyle::PaddingLeft(pt(v)));
    }

    if let Some(v) = b.margin_top {
        styles.push(FlexStyle::MarginTop(pt(v)));
    }
    if let Some(v) = b.margin_right {
        styles.push(FlexStyle::MarginRight(pt(v)));
    }
    if let Some(v) = b.margin_bottom {
        styles.push(FlexStyle::MarginBottom(pt(v)));
    }
    if let Some(v) = b.margin_left {
        styles.push(FlexStyle::MarginLeft(pt(v)));
    }

    if let Some(v) = b.row_gap {
        styles.push(FlexStyle::RowGap(pt(v)));
    }
    if let Some(v) = b.column_gap {
        styles.push(FlexStyle::ColumnGap(pt(v)));
    }

    styles.push(FlexStyle::AlignItems(map_align_items(&b.align_items)));
    styles.push(FlexStyle::AlignSelf(map_align_self(&b.align_self)));
    styles.push(FlexStyle::AlignContent(map_align_content(&b.align_content)));
    styles.push(FlexStyle::JustifyContent(map_justify(&b.justify_content)));

    styles.push(FlexStyle::Position(match b.position {
        InkPosition::Relative => yoga::PositionType::Relative,
        InkPosition::Absolute => yoga::PositionType::Absolute,
    }));

    if let Some(v) = b.top {
        styles.push(FlexStyle::Top(pt(v)));
    }
    if let Some(v) = b.right {
        styles.push(FlexStyle::Right(pt(v)));
    }
    if let Some(v) = b.bottom {
        styles.push(FlexStyle::Bottom(pt(v)));
    }
    if let Some(v) = b.left {
        styles.push(FlexStyle::Left(pt(v)));
    }

    styles.push(FlexStyle::Overflow(match b.overflow_x {
        InkOverflow::Visible => yoga::Overflow::Visible,
        InkOverflow::Hidden => yoga::Overflow::Hidden,
    }));

    let bt = if b.borders.top { 1.0 } else { 0.0 };
    let br = if b.borders.right { 1.0 } else { 0.0 };
    let bb = if b.borders.bottom { 1.0 } else { 0.0 };
    let bl = if b.borders.left { 1.0 } else { 0.0 };
    if bt > 0.0 {
        styles.push(FlexStyle::BorderTop(OrderedFloat(bt)));
    }
    if br > 0.0 {
        styles.push(FlexStyle::BorderRight(OrderedFloat(br)));
    }
    if bb > 0.0 {
        styles.push(FlexStyle::BorderBottom(OrderedFloat(bb)));
    }
    if bl > 0.0 {
        styles.push(FlexStyle::BorderLeft(OrderedFloat(bl)));
    }

    node.apply_styles(&styles);
}

fn apply_text_style(node: &mut yoga::Node, t: &Text) {
    let width = t.content.width() as u16;
    let height = if t.content.is_empty() { 0 } else { 1 };
    node.apply_styles(&[
        yoga::FlexStyle::Width(pt(width)),
        yoga::FlexStyle::Height(pt(height)),
    ]);
}

fn apply_newline_style(node: &mut yoga::Node) {
    node.apply_styles(&[
        yoga::FlexStyle::Width(pt(0)),
        yoga::FlexStyle::Height(pt(1)),
    ]);
}

fn apply_spacer_style(node: &mut yoga::Node) {
    node.apply_styles(&[
        yoga::FlexStyle::FlexGrow(OrderedFloat(1.0)),
        yoga::FlexStyle::Width(auto()),
        yoga::FlexStyle::Height(auto()),
    ]);
}

fn apply_static_style(node: &mut yoga::Node) {
    node.apply_styles(&[
        yoga::FlexStyle::FlexDirection(yoga::FlexDirection::Column),
        yoga::FlexStyle::Width(auto()),
        yoga::FlexStyle::Height(auto()),
    ]);
}

/* -------------------------------------------------------------------------- */
/* Style helpers                                                              */
/* -------------------------------------------------------------------------- */

fn pt(v: u16) -> yoga::StyleUnit {
    yoga::StyleUnit::Point(OrderedFloat(v as f32))
}

fn auto() -> yoga::StyleUnit {
    yoga::StyleUnit::Auto
}

fn pct(v: f32) -> yoga::StyleUnit {
    yoga::StyleUnit::Percent(OrderedFloat(v))
}

fn opt_pt(v: Option<u16>) -> yoga::StyleUnit {
    v.map(pt).unwrap_or_else(auto)
}

fn map_align_items(a: &InkAlignItems) -> yoga::Align {
    match a {
        InkAlignItems::FlexStart => yoga::Align::FlexStart,
        InkAlignItems::Center => yoga::Align::Center,
        InkAlignItems::FlexEnd => yoga::Align::FlexEnd,
        InkAlignItems::Stretch => yoga::Align::Stretch,
        InkAlignItems::Baseline => yoga::Align::Baseline,
    }
}

fn map_align_self(a: &InkAlignSelf) -> yoga::Align {
    match a {
        InkAlignSelf::Auto => yoga::Align::Auto,
        InkAlignSelf::FlexStart => yoga::Align::FlexStart,
        InkAlignSelf::Center => yoga::Align::Center,
        InkAlignSelf::FlexEnd => yoga::Align::FlexEnd,
        InkAlignSelf::Stretch => yoga::Align::Stretch,
        InkAlignSelf::Baseline => yoga::Align::Baseline,
    }
}

fn map_align_content(a: &InkAlignContent) -> yoga::Align {
    match a {
        InkAlignContent::FlexStart => yoga::Align::FlexStart,
        InkAlignContent::Center => yoga::Align::Center,
        InkAlignContent::FlexEnd => yoga::Align::FlexEnd,
        InkAlignContent::Stretch => yoga::Align::Stretch,
        InkAlignContent::SpaceBetween => yoga::Align::SpaceBetween,
        InkAlignContent::SpaceAround => yoga::Align::SpaceAround,
    }
}

fn map_justify(j: &InkJustifyContent) -> yoga::Justify {
    match j {
        InkJustifyContent::FlexStart => yoga::Justify::FlexStart,
        InkJustifyContent::Center => yoga::Justify::Center,
        InkJustifyContent::FlexEnd => yoga::Justify::FlexEnd,
        InkJustifyContent::SpaceBetween => yoga::Justify::SpaceBetween,
        InkJustifyContent::SpaceAround => yoga::Justify::SpaceAround,
        InkJustifyContent::SpaceEvenly => yoga::Justify::SpaceEvenly,
    }
}

/* -------------------------------------------------------------------------- */
/* Absolute rect computation                                                  */
/* -------------------------------------------------------------------------- */

fn compute_absolute(
    entries: &[LayoutEntry],
    nodes: &[yoga::Node],
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

    match entry.node_idx {
        Some(idx) => {
            let layout = nodes[idx].get_layout();
            let x = parent_x + layout.left();
            let y = parent_y + layout.top();
            let w = layout.width();
            let h = layout.height();
            rects.push((x as u16, y as u16, w as u16, h as u16));

            match &entry.vnode.0 {
                VNodeContent::Box(b) if !matches!(b.display, InkDisplay::None) => {
                    for _ in 0..b.children.len() {
                        compute_absolute(entries, nodes, cursor, x, y, rects);
                    }
                }
                VNodeContent::Static(s) => {
                    for _ in 0..s.children.len() {
                        compute_absolute(entries, nodes, cursor, x, y, rects);
                    }
                }
                VNodeContent::Transform(t) => {
                    compute_absolute(entries, nodes, cursor, x, y, rects);
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
                    compute_absolute(entries, nodes, cursor, parent_x, parent_y, rects);
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
    fn yoga_layout_text_in_box() {
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
    fn yoga_row_layout_positions_children_side_by_side() {
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
    fn debug_all_border_styles() {
        use crate::components::Box as InkBox;
        use crate::components::Text;
        let root = InkBox::column()
            .padding(1)
            .child(VNode::from(Text::new("Border Styles Demo")))
            .child({
                let mut b = InkBox::new(); b.margin_top = Some(1);
                b.child(VNode::from(
                    InkBox::new().border_style(crate::style::BorderStyle::Single).padding(1)
                        .child(VNode::from(Text::new("Single border")))
                ))
            })
            .child({
                let mut b = InkBox::new(); b.margin_top = Some(1);
                b.child(VNode::from(
                    InkBox::new().border_style(crate::style::BorderStyle::Double).padding(1)
                        .child(VNode::from(Text::new("Double border")))
                ))
            })
            .into();
        let layout = compute(&root, 80, 24);
        for (i, r) in layout.rects.iter().enumerate() {
            eprintln!("rect[{}] = {:?}", i, r);
        }
    }

    #[test]
    fn yoga_layout_honours_display_none() {
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
