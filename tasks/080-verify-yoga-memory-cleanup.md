# Task 080: Verify Yoga Node Memory Cleanup

## Status: 🟡 **SIGNIFICANT IMPROVEMENT — NOT STARTED**

## Goal
Verify that Yoga C++ nodes are properly freed when InkNode Rust structs are dropped, and fix any leaks.

## Problem

`InkNode` contains a `yoga::Node`, which wraps a C++ Yoga layout node:

```rust
pub struct InkNode {
    pub id: u32,
    pub tag: InkTag,
    pub yoga: Node,  // C++ Yoga node — who frees this?
    // ...
}
```

`destroy_root()` drops the Rust `InkNode` structs by setting them to `None`:

```rust
pub fn destroy_root(&mut self, root_id: u32) {
    if self.root_id == Some(root_id) {
        for node_opt in self.nodes.iter_mut() {
            *node_opt = None;  // Drops InkNode, but does yoga::Node free its C++ pointer?
        }
    }
}
```

If the `yoga` crate (version 0.5) does not implement `Drop` for `Node` to call `YGNodeFree()`, the C++ heap memory leaks every time a tree is destroyed. With hot reload, this could accumulate significantly.

## Investigation Steps

1. Check the `yoga` crate source or docs to see if `Node` implements `Drop`.
2. If it does not, verify by running Valgrind or AddressSanitizer on a test that creates and destroys many trees:

```rust
#[test]
fn test_yoga_memory_cleanup() {
    for _ in 0..10_000 {
        let root = __ink_create_root();
        let child = __ink_create_node("ink-box", "{}").unwrap();
        __ink_append_child(root, child).unwrap();
        __ink_destroy_root(root);
    }
    // If yoga nodes leak, memory usage will climb
}
```

3. If a leak is confirmed, implement `Drop` for `InkNode` or wrap `yoga::Node` in a newtype that calls the free function.

## Fix Approach (if needed)

```rust
// If yoga::Node does NOT implement Drop:
pub struct YogaNode(pub Node);

impl Drop for YogaNode {
    fn drop(&mut self) {
        // yoga crate may expose YGNodeFree or similar
        // If not, we may need to use yoga::Node::free() or bind it ourselves
    }
}

// Then in InkNode:
pub struct InkNode {
    pub yoga: YogaNode,
    // ...
}
```

If the `yoga` crate lacks a free function, we may need to:
- Fork/patch the `yoga` crate to add `Drop`
- Or switch to a Rust-native layout crate (e.g., `taffy`) that has proper RAII

## Acceptance Criteria
- [ ] Confirmed whether `yoga::Node` frees its C++ memory on Rust drop
- [ ] If leak exists: implemented `Drop` or wrapper to free Yoga nodes
- [ ] If leak exists: added test that creates/destroys 10k trees and passes Valgrind/ASan
- [ ] If no leak: documented finding in `src/ink/node.rs` comments
- [ ] `cargo test` passes

## Files to Modify
- `src/ink/node.rs` — Add `Drop` impl or wrapper if needed
- `src/ink/runtime.rs` — Use wrapped type if needed

## References
- yoga crate: https://docs.rs/yoga/latest/yoga/
- Yoga C++ API: https://www.yogalayout.dev/docs/getting-started/laying-a-tree
- taffy (alternative): https://docs.rs/taffy/latest/taffy/
