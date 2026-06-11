//! Ink tree operations
//!
//! Append, remove, insert, and update operations on the node tree.

#[allow(unused_imports)]
use crate::ink::InkNode;
#[allow(unused_imports)]
use crate::ink::InkError;
#[allow(unused_imports)]
use crate::ink::PropValue;
use yoga::Node;

#[allow(unused_imports)]
use std::collections::HashMap;

pub type Result<T> = std::result::Result<T, InkError>;

/// Append child to parent
pub fn append_child(runtime: &mut crate::ink::InkRuntime, parent_id: u32, child_id: u32) -> Result<()> {
    let (child_yoga_ptr, old_parent_id) = {
        let child = runtime
            .get_node(child_id)
            .ok_or(InkError::NodeNotFound(child_id))?;
        let ptr = &child.yoga as *const Node as *mut Node;
        (ptr, child.parent)
    };

    if let Some(old_pid) = old_parent_id {
        if old_pid != parent_id {
            if let Some(old_parent) = runtime.get_node_mut(old_pid) {
                old_parent.children.retain(|&id| id != child_id);
                unsafe {
                    old_parent.yoga.remove_child(&mut *child_yoga_ptr);
                }
            }
        }
    }

    let parent = runtime
        .get_node_mut(parent_id)
        .ok_or(InkError::NodeNotFound(parent_id))?;

    parent.children.push(child_id);

    unsafe {
        parent.yoga.insert_child(&mut *child_yoga_ptr, parent.children.len() - 1);
    }

    if let Some(child_node) = runtime.get_node_mut(child_id) {
        child_node.parent = Some(parent_id);
    }

    runtime.dirty = true;
    Ok(())
}

/// Remove child from parent
pub fn remove_child(runtime: &mut crate::ink::InkRuntime, parent_id: u32, child_id: u32) -> Result<()> {
    let child_ptr = runtime
        .get_node(child_id)
        .map(|c| &c.yoga as *const Node as *mut Node);

    let parent = runtime
        .get_node_mut(parent_id)
        .ok_or(InkError::NodeNotFound(parent_id))?;

    if let Some(ptr) = child_ptr {
        unsafe {
            parent.yoga.remove_child(&mut *ptr);
        }
    }

    parent.children.retain(|&id| id != child_id);

    if let Some(child) = runtime.get_node_mut(child_id) {
        child.parent = None;
    }

    // Free the node's memory so removed nodes don't leak.
    // The Yoga C++ pointer inside InkNode is dropped here.
    runtime.remove_node(child_id);

    runtime.dirty = true;
    Ok(())
}

/// Insert child before another child
pub fn insert_before(
    runtime: &mut crate::ink::InkRuntime,
    parent_id: u32,
    child_id: u32,
    before_id: u32,
) -> Result<()> {
    let (insert_idx, old_parent_id, child_ptrs) =
        gather_insert_info(runtime, parent_id, child_id, before_id)?;

    let parent = runtime
        .get_node_mut(parent_id)
        .ok_or(InkError::NodeNotFound(parent_id))?;

    let old_pid_for_later = if old_parent_id != Some(parent_id) {
        old_parent_id
    } else {
        None
    };

    parent.children.retain(|&id| id != child_id);
    parent.children.insert(insert_idx, child_id);

    reinsert_yoga_children(parent, &child_ptrs);

    if let Some(old_pid) = old_pid_for_later {
        if let Some(old_parent) = runtime.get_node_mut(old_pid) {
            old_parent.children.retain(|&id| id != child_id);
        }
    }

    if let Some(child) = runtime.get_node_mut(child_id) {
        child.parent = Some(parent_id);
    }

    runtime.dirty = true;
    Ok(())
}

#[allow(clippy::type_complexity)]
fn gather_insert_info(
    runtime: &crate::ink::InkRuntime,
    parent_id: u32,
    child_id: u32,
    before_id: u32,
) -> Result<(usize, Option<u32>, Vec<(*mut Node, u32)>)> {
    let parent = runtime
        .get_node(parent_id)
        .ok_or(InkError::NodeNotFound(parent_id))?;
    let insert_idx = parent
        .children
        .iter()
        .position(|&id| id == before_id)
        .ok_or(InkError::InsertBeforeError(before_id))?;
    let old_parent_id = runtime.get_node(child_id).and_then(|c| c.parent);
    let child_ptrs: Vec<(*mut Node, u32)> = parent
        .children
        .iter()
        .filter(|&&id| id != child_id)
        .filter_map(|&id| {
            runtime
                .get_node(id)
                .map(|n| (&n.yoga as *const Node as *mut Node, id))
        })
        .collect();
    Ok((insert_idx, old_parent_id, child_ptrs))
}

fn reinsert_yoga_children(parent: &mut InkNode, child_ptrs: &[(*mut Node, u32)]) {
    for (i, &(ptr, _)) in child_ptrs.iter().enumerate() {
        unsafe {
            parent.yoga.insert_child(&mut *ptr, i);
        }
    }
}

/// Commit an update to a node's props.
/// Short-circuits if the JSON representation is identical to the last one.
pub fn commit_update(
    runtime: &mut crate::ink::InkRuntime,
    node_id: u32,
    props: HashMap<String, PropValue>,
) -> Result<()> {
    let node = runtime
        .get_node_mut(node_id)
        .ok_or(InkError::NodeNotFound(node_id))?;

    // One-shot prop signature comparison (Task 113)
    let json = crate::ink::node::props_to_json(&props);
    let json_bytes = json.as_bytes().to_vec();
    if let Some(ref last) = node.last_props_json {
        if last == &json_bytes {
            return Ok(());
        }
    }

    node.apply_props(&props);
    node.props_json_cache = Some(json);
    node.last_props_json = Some(json_bytes);
    runtime.dirty = true;
    Ok(())
}

/// Set text content of a text node
pub fn set_text(runtime: &mut crate::ink::InkRuntime, node_id: u32, text: &str) -> Result<()> {
    let node = runtime
        .get_node_mut(node_id)
        .ok_or(InkError::NodeNotFound(node_id))?;
    node.text = Some(text.to_string());
    runtime.dirty = true;
    Ok(())
}
