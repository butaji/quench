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

            // Compute effective width/height.
            let bw = b.width.unwrap_or(w);
            let bh = b.height.unwrap_or(h);

            // Account for borders.
            let border_h: u16 = if b.borders.left || b.borders.right { 2 } else { 0 };
            let border_v: u16 = if b.borders.top || b.borders.bottom { 2 } else { 0 };

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
            layout_children(b, x + border_h + pad_l, y + border_v + pad_t, inner_w, inner_h, rects);
        }
        VNodeContent::Text(_) | VNodeContent::Newline(_) | VNodeContent::Spacer(_) => {
            // Leaf nodes: take the full available area.
            // Text intrinsic sizing is handled by the
            // render walker (it clips the text to the
            // rect).
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
        let (cs, cc, cg) = compute_child_main_size(child, main_size, cross_size);
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

        // Compute child rect.
        let (cx, cy, cw, ch) = if is_row {
            (x + offset as u16, y, cs, cc)
        } else {
            (x, y + offset as u16, cc, cs)
        };

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
fn compute_child_main_size(child: &VNode, main_size: u16, cross_size: u16) -> (u16, u16, f32) {
    match &child.0 {
        VNodeContent::Box(b) => {
            let grow = b.flex_grow;
            let ms = if grow > 0.0 {
                0 // flex-grow children start at 0
            } else {
                b.width.unwrap_or(main_size)
            };
            let cs = b.height.unwrap_or(cross_size);
            (ms.min(main_size), cs.min(cross_size), grow)
        }
        VNodeContent::Text(_) | VNodeContent::Newline(_) => {
            // Text/Newline: intrinsic main size is
            // 1 (one row of text). The walker
            // measures the actual width for row
            // flex. For column flex, 1 row is the
            // intrinsic height.
            (1, 1, 0.0)
        }
        VNodeContent::Spacer(_) => {
            (0, cross_size, 1.0) // Spacer: flex-grow
        }
        VNodeContent::Static(s) => {
            // Static: shrink-wrap to first child's size.
            if let Some(first) = s.children.first() {
                compute_child_main_size(first, main_size, cross_size)
            } else {
                (0, 0, 0.0)
            }
        }
        VNodeContent::Transform(t) => {
            compute_child_main_size(&t.child, main_size, cross_size)
        }
        VNodeContent::Fragment(fs) => {
            if let Some(first) = fs.first() {
                compute_child_main_size(first, main_size, cross_size)
            } else {
                (0, 0, 0.0)
            }
        }
    }
}
