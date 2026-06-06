use crate::components::{Box as InkBox, FlexDirection};
use crate::vnode::{VNode, VNodeContent};

pub fn child_flex_grow(node: &VNode) -> f32 {
    match &node.0 {
        VNodeContent::Spacer(_) => 1.0,
        VNodeContent::Box(b) => b.flex_grow,
        _ => 0.0,
    }
}

pub fn measure_intrinsic_main_axis(node: &VNode, dir: FlexDirection) -> u16 {
    if child_flex_grow(node) > 0.0 { return 0; }
    match dir {
        FlexDirection::Row | FlexDirection::RowReverse => measure_intrinsic_width(node),
        FlexDirection::Column | FlexDirection::ColumnReverse => measure_intrinsic_height(node),
    }
}

pub fn compute_preorder_index(children: &[VNode], i: usize, first_child_depth: usize) -> usize {
    let mut depth = first_child_depth;
    for (j, child) in children.iter().enumerate() {
        if j == i { return depth; }
        depth += subtree_size(child);
    }
    depth
}

pub fn subtree_size(node: &VNode) -> usize {
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

pub fn measure_intrinsic_height(node: &VNode) -> u16 {
    match &node.0 {
        VNodeContent::Text(_) | VNodeContent::Newline(_) => 1,
        VNodeContent::Box(b) => b.children.iter().map(measure_intrinsic_height).sum::<u16>(),
        _ => 0,
    }
}

pub fn measure_intrinsic_width(node: &VNode) -> u16 {
    match &node.0 {
        VNodeContent::Text(t) => t.content.chars().count() as u16,
        VNodeContent::Box(b) => {
            if b.children.is_empty() { return 0; }
            let mut total = 0u16;
            for (i, c) in b.children.iter().enumerate() {
                if i > 0 { total = total.saturating_add(1); }
                total = total.saturating_add(measure_intrinsic_width(c));
            }
            total
        }
        _ => 0,
    }
}
