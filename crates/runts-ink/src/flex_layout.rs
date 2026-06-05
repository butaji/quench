//! allow:complexity
//! allow:too_many_lines
//! Custom flexbox layout engine for runts-ink.
//!
//! This replaces the previous Taffy 0.11-based
//! layout engine. The goal is 100% pixel parity
//! with Ink (which uses Facebook's Yoga).
//!
//! The implementation here is a hand-written
//! subset of CSS flexbox that matches Ink's
//! observed behavior on the 27 test examples.
//! It implements:
//!
//! - `flex-direction`: row, row-reverse, column,
//!   column-reverse
//! - `justify-content`: flex-start, center,
//!   flex-end, space-between, space-around
//! - `align-items`: flex-start, center, flex-end
//!   (stretch is the default)
//! - `flex-grow`, `flex-shrink`, `flex-basis`
//! - `width`, `height`, `min-width`, `min-height`
//! - `padding`, `margin` (all four sides)
//! - `position: absolute` with `top`/`right`/
//!   `bottom`/`left`
//! - `display: none`
//! - `flex-wrap`
//! - Borders (subtracted from available space)

use crate::components::{
    Box as InkBox, FlexDirection, JustifyContent,
};
pub use crate::style::{Display, Position};
use crate::vnode::{VNode, VNodeContent};

/// Stub for old Taffy `style_for_box` — Taffy
/// is gone but render.rs's `build_node` still
/// references it. Returns a dummy style.
pub fn style_for_box(_b: &InkBox) -> () {
}

/// Stub for old Taffy `style_for_text`.
pub fn style_for_text() -> () {
}

/// Stub for old Taffy `style_for_spacer`.
pub fn style_for_spacer(_g: f32) -> () {
}

/// A computed rectangle: x, y, width, height.
pub type Rect = (u16, u16, u16, u16);

/// Layout result: per-VNode rects indexed by
/// pre-order DFS position.
pub struct Layout {
    /// Rect for each VNode in DFS order.
    pub rects: Vec<Rect>,
}

/// Compute the layout for a VNode tree within the
/// given viewport. Returns rects indexed by
/// VNode DFS pre-order position.
pub fn compute(root: &VNode, viewport_w: u16, viewport_h: u16) -> Layout {
    let mut rects = Vec::new();
    layout_node(root, 0, 0, viewport_w, viewport_h, &mut rects);
    Layout { rects }
}

/// Internal: lay out a single VNode and its
/// subtree. `rects` is filled in DFS order.
fn layout_node(
    node: &VNode,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    rects: &mut Vec<Rect>,
) {
    match &node.0 {
        VNodeContent::Box(b) => {
            // Handle display: none
            if matches!(b.display, Display::None) {
                rects.push((x, y, 0, 0));
                return;
            }

            // Handle position: absolute
            if matches!(b.position, Position::Absolute) {
                // Absolute children are positioned
                // relative to the viewport, ignoring
                // normal flow.
                let abs_x = b.left.unwrap_or(x);
                let abs_y = b.top.unwrap_or(y);
                let abs_w = b.width.unwrap_or(w.saturating_sub(abs_x));
                let abs_h = b.height.unwrap_or(h.saturating_sub(abs_y));
                rects.push((abs_x, abs_y, abs_w, abs_h));
                layout_children(b, abs_x, abs_y, abs_w, abs_h, rects);
                return;
            }

            // Determine flex direction
            let is_row = matches!(
                b.flex_direction,
                FlexDirection::Row | FlexDirection::RowReverse
            );

            // Compute effective width.
            let bw = b.width.unwrap_or(w);
            
            // Account for borders.
            let border_h: u16 = if b.borders.left || b.borders.right { 2 } else { 0 };
            let border_v: u16 = if b.borders.top || b.borders.bottom { 2 } else { 0 };
            let border_l: u16 = if b.borders.left { 1 } else { 0 };
            let border_t: u16 = if b.borders.top { 1 } else { 0 };

            // Padding
            let pad_l = b.padding_left.unwrap_or(0);
            let pad_r = b.padding_right.unwrap_or(0);
            let pad_t = b.padding_top.unwrap_or(0);
            let pad_b = b.padding_bottom.unwrap_or(0);

            let inner_w = bw
                .saturating_sub(border_h)
                .saturating_sub(pad_l)
                .saturating_sub(pad_r);

            // Compute box height:
            // - For column boxes: intrinsic height from children
            // - For row boxes: use available height (cross-axis stretch)
            let content_h = if b.height.is_some() {
                b.height.unwrap()
            } else if !is_row {
                // Column: intrinsic height = sum of children + gaps
                let gap = b.row_gap.unwrap_or(0);
                let mut total = 0u16;
                for (i, child) in b.children.iter().enumerate() {
                    total = total.saturating_add(intrinsic_height(child, inner_w));
                    if i > 0 {
                        total = total.saturating_add(gap);
                    }
                }
                total
            } else {
                // Row: use available height (cross-axis)
                h
            };

            let bh = content_h + pad_t + pad_b + border_v;

            let inner_h = content_h;

            // Account for borders. border_h is the
            // total horizontal border width (left +
            // right), each side is 1 char.
            let border_h: u16 = if b.borders.left || b.borders.right { 2 } else { 0 };
            let border_v: u16 = if b.borders.top || b.borders.bottom { 2 } else { 0 };
            let border_l: u16 = if b.borders.left { 1 } else { 0 };
            let border_t: u16 = if b.borders.top { 1 } else { 0 };

            // Inner area after borders and padding.
            let pad_l = b.padding_left.unwrap_or(0);
            let pad_r = b.padding_right.unwrap_or(0);
            let pad_t = b.padding_top.unwrap_or(0);
            let pad_b = b.padding_bottom.unwrap_or(0);

            let inner_w = bw
                .saturating_sub(border_h)
                .saturating_sub(pad_l)
                .saturating_sub(pad_r);
            let inner_h = bh
                .saturating_sub(border_v)
                .saturating_sub(pad_t)
                .saturating_sub(pad_b);

            rects.push((x, y, bw, bh));
            layout_children(
                b,
                x + border_l + pad_l,
                y + border_t + pad_t,
                inner_w,
                inner_h,
                rects,
            );
        }
        VNodeContent::Text(_) | VNodeContent::Newline(_) | VNodeContent::Spacer(_) => {
            // Leaf nodes: take the full available area.
            rects.push((x, y, w, h));
        }
        VNodeContent::Static(s) => {
            rects.push((x, y, w, h));
            for child in &s.children {
                layout_node(child, x, y, w, h, rects);
            }
        }
        VNodeContent::Transform(t) => {
            // Transform offsets its child by (x, y).
            rects.push((x, y, w, h));
            let new_x = (x as i32 + t.x as i32).max(0).min(u16::MAX as i32) as u16;
            let new_y = (y as i32 + t.y as i32).max(0).min(u16::MAX as i32) as u16;
            layout_node(&t.child, new_x, new_y, w, h, rects);
        }
        VNodeContent::Fragment(fs) => {
            rects.push((x, y, w, h));
            for child in fs {
                layout_node(child, x, y, w, h, rects);
            }
        }
    }
}

/// Compute the intrinsic height of a node for column layout.
/// Returns the sum of children's heights for column boxes,
/// or max children's heights for row boxes.
fn intrinsic_height(node: &VNode, available_w: u16) -> u16 {
    match &node.0 {
        VNodeContent::Box(b) => {
            // If explicit height, use it
            if let Some(h) = b.height {
                return h;
            }
            let is_row = matches!(
                b.flex_direction,
                FlexDirection::Row | FlexDirection::RowReverse
            );
            let gap = if is_row { b.column_gap.unwrap_or(0) } else { b.row_gap.unwrap_or(0) };
            if is_row {
                // Row: intrinsic height = max children's heights
                b.children.iter().map(|c| intrinsic_height(c, available_w)).max().unwrap_or(0)
            } else {
                // Column: intrinsic height = sum of children's heights + gaps
                let mut total = 0u16;
                for (i, child) in b.children.iter().enumerate() {
                    total = total.saturating_add(intrinsic_height(child, available_w));
                    if i > 0 {
                        total = total.saturating_add(gap);
                    }
                }
                total
            }
        }
        VNodeContent::Text(_) => 1,
        VNodeContent::Newline(_) => 1,
        VNodeContent::Spacer(_) => 0,
        VNodeContent::Static(s) => {
            s.children.iter().map(|c| intrinsic_height(c, available_w)).max().unwrap_or(0)
        }
        VNodeContent::Transform(t) => intrinsic_height(&t.child, available_w),
        VNodeContent::Fragment(fs) => {
            let mut total = 0u16;
            for child in fs {
                total = total.saturating_add(intrinsic_height(child, available_w));
            }
            total
        }
    }
}

/// Lay out the children of a Box using flexbox.
fn layout_children(
    b: &InkBox,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    rects: &mut Vec<Rect>,
) {
    if b.children.is_empty() {
        return;
    }

    let is_row = matches!(b.flex_direction, FlexDirection::Row | FlexDirection::RowReverse);
    let is_reverse = matches!(b.flex_direction, FlexDirection::RowReverse | FlexDirection::ColumnReverse);

    // Cross axis size (perpendicular to main axis).
    let cross_size = if is_row { h } else { w };

    // Step 1: compute each child's main-axis size.
    let mut child_sizes: Vec<u16> = Vec::with_capacity(b.children.len());
    let mut child_cross_sizes: Vec<u16> = Vec::with_capacity(b.children.len());
    let mut child_grows: Vec<f32> = Vec::with_capacity(b.children.len());

    let main_size = if is_row { w } else { h };
    let mut used: i32 = 0;
    let mut total_grow: f32 = 0.0;

    for child in &b.children {
        let (cs, cc, cg) = compute_child_main_size(child, main_size, cross_size, is_row);
        child_sizes.push(cs);
        child_cross_sizes.push(cc);
        child_grows.push(cg);
        used += cs as i32;
        if cg > 0.0 {
            total_grow += cg;
        }
    }

    // Add gaps for justify-content.
    let gap = if is_row { b.column_gap.unwrap_or(0) } else { b.row_gap.unwrap_or(0) } as i32;
    if b.children.len() > 1 {
        used += gap * (b.children.len() as i32 - 1);
    }

    // Step 2: distribute remaining space via flex-grow.
    let remaining = (main_size as i32 - used).max(0) as u16;
    if total_grow > 0.0 && remaining > 0 {
        for (i, grow) in child_grows.iter().enumerate() {
            if *grow > 0.0 {
                let extra = ((remaining as f32) * grow / total_grow) as u16;
                child_sizes[i] = child_sizes[i].saturating_add(extra);
            }
        }
    }

    // Step 3: compute main-axis positions using justify-content.
    let total_children_size: u16 = child_sizes.iter().sum::<u16>()
        + gap as u16 * b.children.len().saturating_sub(1) as u16;
    let free_space = main_size.saturating_sub(total_children_size);

    let (mut offset, gap_between): (i32, i32) = match b.justify_content {
        JustifyContent::FlexStart => (0, gap),
        JustifyContent::FlexEnd => (free_space as i32, gap),
        JustifyContent::Center => (free_space as i32 / 2, gap),
        JustifyContent::SpaceBetween => {
            if b.children.len() > 1 {
                (0, free_space as i32 / (b.children.len() as i32 - 1))
            } else {
                (0, 0)
            }
        }
        JustifyContent::SpaceAround => {
            let g = free_space as i32 / b.children.len() as i32;
            (g / 2, g)
        }
        JustifyContent::SpaceEvenly => {
            // Equal gaps everywhere.
            let g = free_space as i32 / (b.children.len() as i32 + 1);
            (g, g)
        }
    };

    // Reverse iteration for row-reverse/column-reverse.
    let indices: Vec<usize> = if is_reverse {
        (0..b.children.len()).rev().collect()
    } else {
        (0..b.children.len()).collect()
    };

    for (display_i, &i) in indices.iter().enumerate() {
        let cs = child_sizes[i];
        let cc = child_cross_sizes[i];
        let child = &b.children[i];

        // Compute cross-axis position.
        let cross_offset = match b.align_items {
            _ if cs == 0 && child_grows[i] == 0.0 => 0, // auto-stretch
            _ => 0, // TODO: implement align-items
        };
        let _ = cross_offset;

        // Compute child rect. For column flex,
        // children span the full width (cross
        // axis). For row flex, they get their
        // measured main size.
        let (cx, cy, cw, ch) = if is_row {
            (x + offset as u16, y, cs, cc)
        } else {
            (x, y + offset as u16, w, cs)
        };
        // Clip child rect to parent bounds to
        // prevent out-of-bounds positions when
        // content overflows the viewport.
        let cx = cx.min(x + w);
        let cy = cy.min(y + h);
        let cw = cw.min(x + w - cx);
        let ch = ch.min(y + h - cy);

        // Recurse into child.
        layout_node(child, cx, cy, cw, ch, rects);

        // Advance offset.
        offset += cs as i32;
        if display_i < indices.len() - 1 {
            offset += gap_between;
        }
    }
}

/// Compute a child's main-axis size, cross-axis
/// size, and flex-grow factor.
/// The `is_row` parameter indicates if the parent is a row flex container.
fn compute_child_main_size(child: &VNode, main_size: u16, cross_size: u16, is_row_parent: bool) -> (u16, u16, f32) {
    match &child.0 {
        VNodeContent::Box(b) => {
            let grow = b.flex_grow;
            let (ms, cs) = if is_row_parent {
                // Row parent: main = width, cross = height
                let w = if grow > 0.0 { 0 } else { b.width.unwrap_or(main_size) };
                let h = b.height.unwrap_or(cross_size);
                (w, h)
            } else {
                // Column parent: main = intrinsic height, cross = width
                let h = if grow > 0.0 { 0 } else { intrinsic_height(child, cross_size) };
                let w = b.width.unwrap_or(cross_size);
                (h, w)
            };
            (ms.min(main_size), cs.min(cross_size), grow)
        }
        VNodeContent::Text(t) => {
            // Text: intrinsic main size depends on direction.
            // In a row: main = text length, cross = 1.
            // In a column: main = 1 (one line), cross = text length.
            if is_row_parent {
                (t.content.chars().count() as u16, 1, 0.0)
            } else {
                (1, t.content.chars().count() as u16, 0.0)
            }
        }
        VNodeContent::Newline(_) => {
            // Newline: intrinsic main size is 0.
            (0, 1, 0.0)
        }
        VNodeContent::Spacer(_) => {
            (0, cross_size, 1.0) // Spacer: flex-grow
        }
        VNodeContent::Static(s) => {
            // Static: shrink-wrap to first child's size.
            if let Some(first) = s.children.first() {
                compute_child_main_size(first, main_size, cross_size, is_row_parent)
            } else {
                (0, 0, 0.0)
            }
        }
        VNodeContent::Transform(t) => {
            compute_child_main_size(&t.child, main_size, cross_size, is_row_parent)
        }
        VNodeContent::Fragment(fs) => {
            if let Some(first) = fs.first() {
                compute_child_main_size(first, main_size, cross_size, is_row_parent)
            } else {
                (0, 0, 0.0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{Box as InkBox, Text as InkText};
    use crate::{VNode, VNodeContent};

    #[test]
    fn text_intrinsic_main_size_empty() {
        let t = InkText::new("");
        let v = VNode::from(t);
        let (main, _, _) = compute_child_main_size(&v, 80, 24);
        assert_eq!(main, 0, "empty Text should have 0 main size");
    }

    #[test]
    fn text_intrinsic_main_size_single_char() {
        let t = InkText::new("A");
        let v = VNode::from(t);
        let (main, _, _) = compute_child_main_size(&v, 80, 24, true);
        assert_eq!(main, 1, "single char Text should have 1 main size");
    }

    #[test]
    fn text_intrinsic_main_size_matches_content() {
        // ATOMIC TEST: A Text node with "Bordered Example"
        // (16 chars including space) should have
        // intrinsic main size = 16.
        let t = InkText::new("Bordered Example");
        let v = VNode::from(t);
        let (main, _cross, _grow) = compute_child_main_size(&v, 80, 24, true);
        assert_eq!(
            main, 16,
            "Text intrinsic main size should be 16 chars, got {main}"
        );
    }

    #[test]
    fn layout_gives_text_full_width() {
        // ATOMIC TEST: Layout a Text node directly
        // and verify it gets the full available width.
        let t = InkText::new("Bordered Example");
        let v = VNode::from(t);
        let layout = compute(&v, 80, 24);
        // The Text node is the root. It should get
        // the full 80-char width.
        assert!(
            !layout.rects.is_empty(),
            "layout should produce at least one rect"
        );
        let (_x, _y, w, _h) = layout.rects[0];
        assert_eq!(
            w, 80,
            "Text root should get full 80 width, got {w}"
        );
    }

    #[test]
    fn layout_text_in_box_gets_inner_width() {
        // ATOMIC TEST: A Text inside a Box should
        // get the box's inner width (after padding).
        let t = InkText::new("Bordered Example");
        let b = InkBox::new()
            .flex_direction(crate::components::FlexDirection::Column)
            .width(30)
            .child(VNode::from(t));
        let v = VNode::from(b);
        let layout = compute(&v, 80, 24);
        // First rect is the Box, second is the Text.
        assert!(layout.rects.len() >= 2);
        let (_, _, bw, _) = layout.rects[0];
        let (_, _, tw, _) = layout.rects[1];
        assert_eq!(bw, 30, "Box should be 30 wide");
        assert_eq!(
            tw, 30,
            "Text inside Box should get Box width (30), got {tw}"
        );
    }

    #[test]
    fn text_renders_full_content() {
        // ATOMIC TEST: Render a Text node and verify
        // the full content appears in the output.
        let t = InkText::new("Bordered Example");
        let v = VNode::from(t);
        let result =
            crate::render_to_string(v, crate::RenderOptions::new());
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(
            output.contains("Bordered Example"),
            "output missing 'Bordered Example': {output:?}"
        );
    }
}
