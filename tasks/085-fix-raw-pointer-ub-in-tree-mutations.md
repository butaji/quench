# Task 085: Fix Raw Pointer Undefined Behavior in Tree Mutations

## Status: 🔴 **CRITICAL SAFETY BUG — NOT STARTED**

## Goal
Eliminate undefined behavior in `src/ink/tree.rs` caused by deriving raw pointers from shared borrows, dropping those borrows, then taking mutable borrows and dereferencing the raw pointers.

## Problem

`append_child`, `remove_child`, and `insert_before` all derive raw `*mut Node` pointers from `&InkNode` shared borrows, then use those pointers after taking `&mut InkNode` mutable borrows:

```rust
// src/ink/tree.rs::append_child
let child = runtime.get_node(child_id).ok_or(...)?;  // &InkNode (shared borrow)
let ptr = &child.yoga as *const Node as *mut Node;     // raw pointer derived from shared borrow
// shared borrow DROPPED here ...

let parent = runtime.get_node_mut(parent_id).ok_or(...)?; // &mut InkNode (mutable borrow)
unsafe { parent.yoga.insert_child(&mut *ptr, ...); }       // UB under Stacked Borrows / Tree Borrows
```

`insert_before` is the worst case — it collects raw pointers to **all** children while holding a shared borrow on the parent, then mutably borrows the parent and uses those pointers.

### Why This Is UB

Under Rust's aliasing model (Stacked Borrows / Tree Borrows / strict provenance), a raw pointer derived from a shared reference (`&T`) cannot be used to mutate the referenced data after a mutable reference (`&mut T`) to the same data is created. The mutable reference invalidates the provenance of the shared-derived pointer.

### Impact

- Miri reports UB on this pattern
- Future Rust compiler versions may miscompile this code
- Memory corruption or runtime crashes on aggressive optimization levels

## Fix Approaches

### Option A: Use Yoga Node IDs Instead of Raw Pointers

Restructure to use `yoga::Node` index-based API (if available) or store node handles instead of raw pointers.

### Option B: Restructure Borrows

Collect all needed data before taking any mutable borrows, then perform mutations without raw pointers:

```rust
pub fn append_child(runtime: &mut InkRuntime, parent_id: u32, child_id: u32) -> Result<()> {
    // Phase 1: Collect all data under shared borrow
    let (child_parent, child_exists) = {
        let child = runtime.get_node(child_id).ok_or(InkError::NodeNotFound(child_id))?;
        (child.parent, true)
    };
    
    // Phase 2: Remove from old parent (if needed)
    if let Some(old_pid) = child_parent {
        if old_pid != parent_id {
            remove_child(runtime, old_pid, child_id)?;
        }
    }
    
    // Phase 3: Mutate parent and child
    {
        let parent = runtime.get_node_mut(parent_id).ok_or(...)?;
        parent.children.push(child_id);
        // Use safe Yoga API instead of raw pointers
    }
    {
        let child = runtime.get_node_mut(child_id).ok_or(...)?;
        child.parent = Some(parent_id);
    }
    
    runtime.dirty = true;
    Ok(())
}
```

### Option C: Use `unsafe` With `ManuallyDrop` or `MaybeUninit`

If Yoga absolutely requires raw pointers, use `ptr::addr_of_mut!` from a pinned/owned location, never from a temporary shared borrow.

## Acceptance Criteria
- [ ] Miri passes on all tree mutation functions with `--strict-provenance`
- [ ] `append_child`, `remove_child`, `insert_before` have no raw pointer derived from shared borrows
- [ ] All existing tests pass
- [ ] Add Miri CI check to prevent regression

## Files to Modify
- `src/ink/tree.rs` — Restructure tree mutations

## References
- Rustonomicon on raw pointers: https://doc.rust-lang.org/nomicon/raw-pointers.html
- Stacked Borrows: https://github.com/rust-lang/unsafe-code-guidelines/blob/master/wip/stacked-borrows.md
- Tree Borrows: https://github.com/rust-lang/unsafe-code-guidelines/blob/master/wip/tree-borrows.md
