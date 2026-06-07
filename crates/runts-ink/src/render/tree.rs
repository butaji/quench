use ratatui::layout::Rect;
use crate::components::{Box as InkBox, FlexDirection, JustifyContent, Text, Transform};
use crate::vnode::{VNode, VNodeContent};
use super::color;
use super::layout::Layout;

/// Render a VNode tree to a Ratatui `Frame`.
pub fn render_tree(node: &VNode, layout: &Layout, frame: &mut ratatui::Frame, area: Rect) {
    let root_rect = rect_at(&layout.rects, 0, area);
    let clipped = intersect_rect(area, root_rect);
    walk(node, layout, frame, clipped, 0);
}

fn intersect_rect(a: Rect, b: Rect) -> Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = a.x.saturating_add(a.width).min(b.x.saturating_add(b.width));
    let bottom = a.y.saturating_add(a.height).min(b.y.saturating_add(b.height));
    if right <= x || bottom <= y {
        Rect { x, y, width: 0, height: 0 }
    } else {
        Rect { x, y, width: right - x, height: bottom - y }
    }
}

fn walk(node: &VNode, layout: &Layout, frame: &mut ratatui::Frame, area: Rect, depth: usize) {
    match &node.0 {
        VNodeContent::Box(b) => walk_box(b, layout, frame, area, depth),
        VNodeContent::Text(t) => render_text(t, frame, area),
        VNodeContent::Newline(_) => frame.render_widget(ratatui::widgets::Paragraph::new(""), area),
        VNodeContent::Spacer(_) => {}
        VNodeContent::Static(s) => walk_children(s.children.as_slice(), layout, frame, area, depth + 1, FlexDirection::Row, JustifyContent::FlexStart),
        VNodeContent::Transform(t) => walk_transform(t, layout, frame, area, depth),
        VNodeContent::Fragment(fs) => walk_children(fs.as_slice(), layout, frame, area, depth + 1, FlexDirection::Row, JustifyContent::FlexStart),
    }
    let _ = (layout, depth);
}

fn walk_box(b: &InkBox, layout: &Layout, frame: &mut ratatui::Frame, area: Rect, depth: usize) {
    if matches!(b.display, crate::style::Display::None) { return; }
    if let Some(ref bg) = b.background_color {
        let bg_style = ratatui::style::Style::default().bg(color::color_to_ratatui(bg));
        frame.render_widget(ratatui::widgets::Paragraph::new("").style(bg_style), area);
    }
    if b.borders.top || b.borders.right || b.borders.bottom || b.borders.left {
        let block = build_block(b);
        frame.render_widget(block, area);
    }
    walk_children(b.children.as_slice(), layout, frame, area, depth + 1, b.flex_direction, b.justify_content);
}

fn walk_transform(t: &Transform, layout: &Layout, frame: &mut ratatui::Frame, area: Rect, depth: usize) {
    let _ = (t, area);
    walk(&t.child, layout, frame, area, depth + 1);
}

fn walk_children(children: &[VNode], layout: &Layout, frame: &mut ratatui::Frame, area: Rect, first_child_depth: usize, _parent_flex_dir: FlexDirection, _parent_justify: JustifyContent) {
    for (i, child) in children.iter().enumerate() {
        let child_depth = compute_preorder_index(children, i, first_child_depth);
        let child_area = rect_at(&layout.rects, child_depth, area);
        walk(child, layout, frame, child_area, child_depth);
    }
}

fn compute_preorder_index(children: &[VNode], i: usize, first_child_depth: usize) -> usize {
    let mut depth = first_child_depth;
    for (j, child) in children.iter().enumerate() {
        if j == i { return depth; }
        depth += subtree_size(child);
    }
    depth
}

fn subtree_size(node: &VNode) -> usize {
    if let VNodeContent::Box(b) = &node.0 {
        if matches!(b.display, crate::style::Display::None) { return 0; }
    }
    let mut count = 1;
    match &node.0 {
        VNodeContent::Box(b) => count += b.children.iter().map(subtree_size).sum::<usize>(),
        VNodeContent::Static(s) => count += s.children.iter().map(subtree_size).sum::<usize>(),
        VNodeContent::Fragment(fs) => count += fs.iter().map(subtree_size).sum::<usize>(),
        VNodeContent::Transform(t) => count += subtree_size(&t.child),
        _ => {}
    }
    count
}

fn rect_at(rects: &[(u16, u16, u16, u16)], depth: usize, fallback: Rect) -> Rect {
    if let Some(&(x, y, w, h)) = rects.get(depth) {
        if w == 0 && h == 0 { return fallback; }
        let max_x = fallback.x.saturating_add(fallback.width);
        let max_y = fallback.y.saturating_add(fallback.height);
        Rect { x: x.min(max_x.saturating_sub(1)), y: y.min(max_y.saturating_sub(1)), width: w.min(max_x.saturating_sub(x)).max(1), height: h.min(max_y.saturating_sub(y)).max(1) }
    } else { fallback }
}

fn build_block(b: &InkBox) -> ratatui::widgets::Block<'_> {
    let mut block = ratatui::widgets::Block::default().borders(b.borders.to_ratatui()).border_type(b.border_style.to_ratatui());
    if let Some(ref c) = b.border_color {
        let style = ratatui::style::Style::default().fg(color::color_to_ratatui(c));
        block = if b.border_dim_color { block.border_style(style.add_modifier(ratatui::style::Modifier::DIM)) } else { block.border_style(style) };
    }
    if let Some(ref c) = b.border_background_color {
        block = block.border_style(ratatui::style::Style::default().bg(color::color_to_ratatui(c)));
    }
    block
}

fn render_text(t: &Text, frame: &mut ratatui::Frame, area: Rect) {
    use ratatui::style::{Modifier, Style};
    use ratatui::text::Span;
    let style = if t.has_styling() {
        let mut s = Style::default().fg(color::color_to_ratatui(&t.color)).bg(color::color_to_ratatui(&t.background_color));
        if t.bold { s = s.add_modifier(Modifier::BOLD); }
        if t.italic { s = s.add_modifier(Modifier::ITALIC); }
        if t.underline { s = s.add_modifier(Modifier::UNDERLINED); }
        if t.strikethrough { s = s.add_modifier(Modifier::CROSSED_OUT); }
        if t.dim_color { s = s.add_modifier(Modifier::DIM); }
        if t.inverse { s = s.add_modifier(Modifier::REVERSED); }
        s
    } else { Style::default() };
    frame.render_widget(ratatui::widgets::Paragraph::new(Span::styled(t.content.clone(), style)).wrap(t.wrap.to_ratatui()), area);
}
