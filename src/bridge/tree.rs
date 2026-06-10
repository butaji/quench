//! Bridge: Tree mutations
//!
//! Functions for appending, removing, inserting nodes.

use crate::ink::{INK_RUNTIME, PropValue};
use crate::bridge::props::parse_props_json;

/// Append child to parent
pub fn __ink_append_child(parent_id: u32, child_id: u32) -> crate::bridge::Result<()> {
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        crate::ink::append_child(&mut r, parent_id, child_id)
            .map_err(crate::bridge::FfiError::from)
    })
}

/// Remove child from parent
pub fn __ink_remove_child(parent_id: u32, child_id: u32) -> crate::bridge::Result<()> {
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        crate::ink::remove_child(&mut r, parent_id, child_id)
            .map_err(crate::bridge::FfiError::from)
    })
}

/// Insert child before another child
pub fn __ink_insert_before(
    parent_id: u32,
    child_id: u32,
    before_id: u32,
) -> crate::bridge::Result<()> {
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        crate::ink::insert_before(&mut r, parent_id, child_id, before_id)
            .map_err(crate::bridge::FfiError::from)
    })
}

/// Commit prop updates to a node
pub fn __ink_commit_update(
    node_id: u32,
    props_json: &str,
) -> crate::bridge::Result<()> {
    let props = parse_props_json(props_json)?;
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        crate::ink::commit_update(&mut r, node_id, props)
            .map_err(crate::bridge::FfiError::from)
    })
}

/// Set text content of a text node
pub fn __ink_set_text(node_id: u32, text: &str) -> crate::bridge::Result<()> {
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        crate::ink::set_text(&mut r, node_id, text)
            .map_err(crate::bridge::FfiError::from)
    })
}

/// Mark the tree as needing layout and render
pub fn __ink_commit() {
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        r.mark_dirty()
    })
}

/// Check if the tree is dirty
pub fn __ink_is_dirty() -> bool {
    INK_RUNTIME.with(|runtime| runtime.borrow().is_dirty())
}

/// Clear the dirty flag
pub fn __ink_clear_dirty() {
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        r.clear_dirty()
    })
}

/// Get root ID
pub fn __ink_get_root_id() -> Option<u32> {
    INK_RUNTIME.with(|runtime| runtime.borrow().root_id())
}

/// Calculate layout for the entire tree
pub fn __ink_calculate_layout() -> crate::bridge::Result<()> {
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        r.calculate_layout()
            .map_err(crate::bridge::FfiError::from)
    })
}

// ===================================================================
// Node accessors (for rendering)
// ===================================================================

/// Get node tag
pub fn __ink_get_node_tag(node_id: u32) -> Option<String> {
    INK_RUNTIME.with(|runtime| {
        runtime.borrow().node(node_id).map(|n| match n.tag {
            crate::ink::InkTag::Box => "ink-box".to_string(),
            crate::ink::InkTag::Text => "ink-text".to_string(),
            crate::ink::InkTag::Static => "ink-static".to_string(),
            crate::ink::InkTag::Newline => "ink-newline".to_string(),
            crate::ink::InkTag::Spacer => "ink-spacer".to_string(),
            crate::ink::InkTag::Unknown => "unknown".to_string(),
        })
    })
}

/// Get node text content
pub fn __ink_get_node_text(node_id: u32) -> Option<String> {
    INK_RUNTIME.with(|runtime| {
        runtime.borrow().node(node_id).and_then(|n| n.text.clone())
    })
}

/// Get node children
pub fn __ink_get_node_children(node_id: u32) -> Option<Vec<u32>> {
    INK_RUNTIME.with(|runtime| {
        runtime.borrow().node(node_id).map(|n| n.children.clone())
    })
}

/// Get node parent
pub fn __ink_get_node_parent(node_id: u32) -> Option<u32> {
    INK_RUNTIME.with(|runtime| runtime.borrow().node(node_id).and_then(|n| n.parent))
}

/// Get node prop
pub fn __ink_get_node_prop(node_id: u32, prop: &str) -> Option<String> {
    INK_RUNTIME.with(|runtime| {
        runtime
            .borrow()
            .node(node_id)
            .and_then(|n| n.props.get(prop))
            .map(crate::bridge::node::prop_value_to_json)
    })
}

/// Get raw node prop value
pub fn __ink_get_node_prop_raw(node_id: u32, prop: &str) -> Option<PropValue> {
    INK_RUNTIME.with(|runtime| {
        runtime
            .borrow()
            .node(node_id)
            .and_then(|n| n.props.get(prop).cloned())
    })
}

/// Get the computed layout for a node
pub fn __ink_get_layout(node_id: u32) -> Option<(f32, f32, f32, f32)> {
    INK_RUNTIME.with(|runtime| {
        runtime.borrow().node(node_id).map(|n| {
            let layout = n.get_layout();
            (layout.left(), layout.top(), layout.width(), layout.height())
        })
    })
}

/// Measure element dimensions from Yoga layout
pub fn __ink_measure_element(node_id: u32) -> Option<(f32, f32)> {
    INK_RUNTIME.with(|runtime| {
        let r = runtime.borrow();
        r.node(node_id).map(|n| {
            let layout = n.get_layout();
            (layout.width(), layout.height())
        })
    })
}
