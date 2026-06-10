# Task 078: Switch Node Storage from Sparse Vec to HashMap

## Status: 🟡 **SIGNIFICANT IMPROVEMENT — NOT STARTED**

## Goal
Replace `Vec<Option<InkNode>>` with `HashMap<u32, InkNode>` (or `SlotMap`) for O(1) insertion and no sparse array waste.

## Problem

`src/ink/runtime.rs` stores nodes in a sparse vector:

```rust
pub struct InkRuntime {
    pub(crate) nodes: Vec<Option<InkNode>>,
    // ...
}

// On node creation:
while self.nodes.len() <= id as usize {
    self.nodes.push(None);  // O(n) growth for sparse IDs!
}
self.nodes[id as usize] = Some(node);
```

With node ID 10,000, this pushes 10,000 `None` values into the Vec. Memory usage grows linearly with the maximum ID, not the actual node count.

This is especially bad because:
- `destroy_root()` clears the Vec but doesn't shrink it — memory is never released
- `node()` and `node_mut()` do `vec.get(id as usize)` which is bounds-checked every time
- Iterating over all nodes (if ever needed) requires skipping `None` entries

## Fix Approach

Replace with `HashMap<u32, InkNode>`:

```rust
pub struct InkRuntime {
    pub(crate) nodes: HashMap<u32, InkNode>,
    // ...
}

pub fn create_node(&mut self, tag: &str, props: HashMap<String, PropValue>) -> u32 {
    let id = self.next_id;
    self.next_id += 1;
    let tag = InkTag::from_str(tag);
    let mut node = InkNode::new(id, tag);
    node.apply_props(&props);
    self.nodes.insert(id, node);  // O(1), no sparse allocation
    self.dirty = true;
    id
}

pub(crate) fn get_node(&self, id: u32) -> Option<&InkNode> {
    self.nodes.get(&id)  // O(1)
}
```

### Alternative: SlotMap

`slotmap::SlotMap` provides dense storage with stable IDs and automatic reuse:

```rust
use slotmap::{SlotMap, DefaultKey};

pub struct InkRuntime {
    pub(crate) nodes: SlotMap<DefaultKey, InkNode>,
    pub(crate) root_id: Option<DefaultKey>,
}
```

**Pros:** Dense memory, O(1) insert/remove/lookup, keys are typed (not raw `u32`).
**Cons:** Requires `slotmap` dependency; changes all `u32` node IDs to `DefaultKey`.

**Recommendation:** Use `HashMap<u32, InkNode>` for minimal API churn. SlotMap is better long-term but requires touching every module.

## Performance Impact

| Metric | Vec (sparse) | HashMap | Improvement |
|--------|-------------|---------|-------------|
| Insert (node 10,000) | O(n) growth | O(1) | **Eliminates linear growth** |
| Lookup | O(1) + bounds check | O(1) | Similar |
| Memory (100 nodes, ID 10k) | ~10k slots | ~100 entries | **~100x** memory reduction |
| Destroy root | O(n) clear | O(n) drain | Similar |

## Acceptance Criteria
- [ ] `InkRuntime.nodes` is `HashMap<u32, InkNode>`
- [ ] All `get_node` / `get_node_mut` / `node` / `node_mut` methods use HashMap
- [ ] `destroy_root()` uses `self.nodes.clear()` (which frees memory properly)
- [ ] No `while nodes.len() <= id` pattern remains
- [ ] All tests pass
- [ ] `cargo clippy` clean

## Files to Modify
- `src/ink/runtime.rs` — Replace `Vec` with `HashMap`
- `src/ink/tree.rs` — Update any direct `Vec` indexing

## References
- HashMap docs: https://doc.rust-lang.org/std/collections/struct.HashMap.html
- Task 002 (Bridge: Root Node Lifecycle — original storage design)
