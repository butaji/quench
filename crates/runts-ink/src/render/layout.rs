use crate::components::Box as InkBox;
use crate::flex_layout;
use crate::vnode::VNode;

/// Available space for Yoga layout computation.
pub enum AvailableSpace {
    /// A definite size in points.
    Definite(f32),
}

/// A 2D size with available-space semantics.
#[derive(Debug, Clone, Copy)]
pub struct Size<S = AvailableSpace> {
    /// Width.
    pub width: S,
    /// Height.
    pub height: S,
}

/// A layout computation result using Yoga.
/// Stored after layout computation so the renderer
/// can look up per-node rects.
pub struct Layout {
    /// Per-VNode-index rect. Indexed by VNode
    /// pre-order DFS position. The renderer walks
    /// the VNode tree in the same DFS order, so
    /// index N in `walk`'s `depth` counter lines
    /// up with index N here.
    pub rects: Vec<(u16, u16, u16, u16)>,
    /// The root VNode, stored so `YogaTree::compute`
    /// can re-walk it with the viewport size.
    pub root_vnode: Option<VNode>,
}

impl Default for Layout {
    fn default() -> Self {
        Self::new()
    }
}

impl Layout {
    /// Build a fresh, empty layout state.
    pub fn new() -> Self {
        Self {
            rects: Vec::new(),
            root_vnode: None,
        }
    }
}

/// The Yoga-based layout tree built from a VNode tree.
///
/// This is a thin wrapper around `flex_layout::compute`.
pub struct YogaTree {
    _priv: (),
}

impl YogaTree {
    /// Build a layout tree from a VNode tree. The
    /// result is a `YogaTree` whose only job is
    /// to populate `layout.rects` via the yoga-based
    /// flexbox engine.
    pub fn from_vnode(root: &VNode, layout: &mut Layout) -> Self {
        // Stash the root VNode for `compute` to
        // re-walk with the viewport size.
        layout.root_vnode = Some(root.clone());
        // Pre-allocate one placeholder rect per VNode.
        // The values are filled in by `compute`.
        let node_count = count_vnodes(root);
        layout.rects = vec![(0, 0, 0, 0); node_count];
        Self { _priv: () }
    }

    /// Compute the layout with the given viewport.
    /// Uses the yoga-based flexbox engine in
    /// `crate::flex_layout`.
    pub fn compute(
        &self,
        layout: &mut Layout,
        viewport: Size<AvailableSpace>,
    ) {
        let w = match viewport.width {
            AvailableSpace::Definite(v) => v,
        };
        let h = match viewport.height {
            AvailableSpace::Definite(v) => v,
        };
        // Recompute the layout using Yoga.
        if let Some(root_node) = layout.root_vnode.as_ref() {
            let yoga_layout = flex_layout::compute(root_node, w as u16, h as u16);
            layout.rects = yoga_layout.rects.iter()
                .map(|r| (r.0, r.1, r.2, r.3))
                .collect();
        }
        // Suppress unused warnings (viewport is informational).
        let _ = (w, h);
    }
}

fn count_vnodes(node: &VNode) -> usize {
    use crate::vnode::VNodeContent;
    if matches!(
        &node.0,
        VNodeContent::Box(InkBox { display: crate::style::Display::None, .. })
    ) {
        return 1;
    }
    let mut count = 1;
    match &node.0 {
        VNodeContent::Box(b) => count += count_box_children(&b.children),
        VNodeContent::Static(s) => count += count_static_children(&s.children),
        VNodeContent::Transform(t) => count += count_vnodes(&t.child),
        VNodeContent::Fragment(fs) => count += count_fragment_children(fs),
        _ => {}
    };
    count
}

fn count_box_children(children: &[VNode]) -> usize {
    children.iter().map(count_vnodes).sum()
}

fn count_static_children(children: &[VNode]) -> usize {
    children.iter().map(count_vnodes).sum()
}

fn count_fragment_children(children: &[VNode]) -> usize {
    children.iter().map(count_vnodes).sum()
}
