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
pub fn compute(root: &VNode, viewport_w: u16, _viewport_h: u16) -> Layout {
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

fn apply_box_style(node: &mut yoga::Node, b: &InkBox) {
    use yoga::FlexStyle; let mut styles: Vec<FlexStyle> = Vec::new();
    styles.push(FlexStyle::Display(map_display(&b.display)));
    styles.extend(collect_flex_styles(b));
    styles.extend(collect_dimension_styles(b));
    styles.extend(collect_padding_styles(b));
    styles.extend(collect_margin_styles(b));
    styles.extend(collect_gap_styles(b));
    styles.extend(collect_alignment_styles(b));
    styles.push(FlexStyle::Position(map_position(&b.position)));
    styles.extend(collect_position_offsets(b));
    styles.push(FlexStyle::Overflow(map_overflow(&b.overflow_x)));
    styles.extend(collect_border_styles(b));
    node.apply_styles(&styles);
}

fn map_display(d: &InkDisplay) -> yoga::Display {
    match d { InkDisplay::Flex => yoga::Display::Flex, InkDisplay::None => yoga::Display::None }
}

fn collect_flex_styles(b: &InkBox) -> Vec<yoga::FlexStyle> {
    use yoga::FlexStyle; vec![
        FlexStyle::FlexDirection(map_flex_direction(&b.flex_direction)),
        FlexStyle::FlexWrap(map_flex_wrap(&b.flex_wrap)),
        FlexStyle::FlexGrow(OrderedFloat(b.flex_grow)),
        FlexStyle::FlexShrink(OrderedFloat(b.flex_shrink)),
        FlexStyle::FlexBasis(if b.flex_basis_pct > 0.0 { pct(b.flex_basis_pct) } else { auto() }),
    ]
}

fn map_flex_direction(d: &InkFlexDirection) -> yoga::FlexDirection {
    match d {
        InkFlexDirection::Row => yoga::FlexDirection::Row,
        InkFlexDirection::Column => yoga::FlexDirection::Column,
        InkFlexDirection::RowReverse => yoga::FlexDirection::RowReverse,
        InkFlexDirection::ColumnReverse => yoga::FlexDirection::ColumnReverse,
    }
}

fn map_flex_wrap(w: &InkFlexWrap) -> yoga::Wrap {
    match w { InkFlexWrap::NoWrap => yoga::Wrap::NoWrap, InkFlexWrap::Wrap => yoga::Wrap::Wrap, InkFlexWrap::WrapReverse => yoga::Wrap::WrapReverse }
}

fn collect_dimension_styles(b: &InkBox) -> Vec<yoga::FlexStyle> {
    use yoga::FlexStyle; vec![
        FlexStyle::Width(opt_pt(b.width)),
        FlexStyle::Height(opt_pt(b.height)),
        FlexStyle::MinWidth(opt_pt(b.min_width)),
        FlexStyle::MinHeight(opt_pt(b.min_height)),
        FlexStyle::MaxWidth(opt_pt(b.max_width)),
        FlexStyle::MaxHeight(opt_pt(b.max_height)),
    ]
}

fn collect_padding_styles(b: &InkBox) -> Vec<yoga::FlexStyle> {
    use yoga::FlexStyle; let mut styles = Vec::new();
    if let Some(v) = b.padding_top { styles.push(FlexStyle::PaddingTop(pt(v))); }
    if let Some(v) = b.padding_right { styles.push(FlexStyle::PaddingRight(pt(v))); }
    if let Some(v) = b.padding_bottom { styles.push(FlexStyle::PaddingBottom(pt(v))); }
    if let Some(v) = b.padding_left { styles.push(FlexStyle::PaddingLeft(pt(v))); }
    styles
}

fn collect_margin_styles(b: &InkBox) -> Vec<yoga::FlexStyle> {
    use yoga::FlexStyle; let mut styles = Vec::new();
    if let Some(v) = b.margin_top { styles.push(FlexStyle::MarginTop(pt(v))); }
    if let Some(v) = b.margin_right { styles.push(FlexStyle::MarginRight(pt(v))); }
    if let Some(v) = b.margin_bottom { styles.push(FlexStyle::MarginBottom(pt(v))); }
    if let Some(v) = b.margin_left { styles.push(FlexStyle::MarginLeft(pt(v))); }
    styles
}

fn collect_gap_styles(b: &InkBox) -> Vec<yoga::FlexStyle> {
    use yoga::FlexStyle; let mut styles = Vec::new();
    if let Some(v) = b.row_gap { styles.push(FlexStyle::RowGap(pt(v))); }
    if let Some(v) = b.column_gap { styles.push(FlexStyle::ColumnGap(pt(v))); }
    styles
}

fn collect_alignment_styles(b: &InkBox) -> Vec<yoga::FlexStyle> {
    use yoga::FlexStyle; vec![
        FlexStyle::AlignItems(map_align_items(&b.align_items)),
        FlexStyle::AlignSelf(map_align_self(&b.align_self)),
        FlexStyle::AlignContent(map_align_content(&b.align_content)),
        FlexStyle::JustifyContent(map_justify(&b.justify_content)),
    ]
}

fn map_position(p: &InkPosition) -> yoga::PositionType {
    match p { InkPosition::Relative => yoga::PositionType::Relative, InkPosition::Absolute => yoga::PositionType::Absolute }
}

fn collect_position_offsets(b: &InkBox) -> Vec<yoga::FlexStyle> {
    use yoga::FlexStyle; let mut styles = Vec::new();
    if let Some(v) = b.top { styles.push(FlexStyle::Top(pt(v))); }
    if let Some(v) = b.right { styles.push(FlexStyle::Right(pt(v))); }
    if let Some(v) = b.bottom { styles.push(FlexStyle::Bottom(pt(v))); }
    if let Some(v) = b.left { styles.push(FlexStyle::Left(pt(v))); }
    styles
}

fn map_overflow(o: &InkOverflow) -> yoga::Overflow {
    match o { InkOverflow::Visible => yoga::Overflow::Visible, InkOverflow::Hidden => yoga::Overflow::Hidden }
}

fn collect_border_styles(b: &InkBox) -> Vec<yoga::FlexStyle> {
    use yoga::FlexStyle; let mut styles = Vec::new();
    let bt = if b.borders.top { 1.0 } else { 0.0 };
    let br = if b.borders.right { 1.0 } else { 0.0 };
    let bb = if b.borders.bottom { 1.0 } else { 0.0 };
    let bl = if b.borders.left { 1.0 } else { 0.0 };
    if bt > 0.0 { styles.push(FlexStyle::BorderTop(OrderedFloat(bt))); }
    if br > 0.0 { styles.push(FlexStyle::BorderRight(OrderedFloat(br))); }
    if bb > 0.0 { styles.push(FlexStyle::BorderBottom(OrderedFloat(bb))); }
    if bl > 0.0 { styles.push(FlexStyle::BorderLeft(OrderedFloat(bl))); }
    styles
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Box as InkBox;
    include!("yoga_tests.inc");
}
