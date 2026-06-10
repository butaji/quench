# Task 087: Fix RefCell Reentrancy Panic Risk Across FFI

## Status: 🔴 **CRITICAL BUG — NOT STARTED**

## Goal
Prevent RefCell `already borrowed` panics when JS callbacks trigger nested tree mutations.

## Problem

`INK_RUNTIME` is a `thread_local! { RefCell<InkRuntime> }`. Every bridge call does `runtime.borrow_mut()`. If a JS callback (timer, key handler) triggers a nested tree mutation while the runtime is already borrowed, this panics:

```rust
// Scenario:
// 1. Event loop: __ink_process_timers() → TIMERS.lock() → ctx.eval("__tb_invoke_timers()")
// 2. JS timer callback fires, calls __ink_create_node()
// 3. __ink_create_node → INK_RUNTIME.with(|r| r.borrow_mut()) 
// 4. PANIC: already borrowed mutably
```

The panic happens across the JS→Rust FFI boundary, producing an opaque "already borrowed" message with no stack trace.

## Why This Can Happen

- Timer dispatch: `poll_timers()` → `ctx.eval("__tb_invoke_timers()")` → JS callback → `__ink_call("create_node")` → `borrow_mut()` while timer system may also hold runtime borrow
- Key/mouse dispatch: `handle_key_event()` → `ctx.eval("__tb_dispatch_key()")` → JS handler → `__ink_call` → `borrow_mut()`
- `render_tree()` is called from event loop while JS callbacks may fire

## Fix Approaches

### Option A: Queue Mutations (Recommended)

Instead of executing tree mutations immediately, queue them and apply after releasing all borrows:

```rust
static MUTATION_QUEUE: LazyLock<Mutex<Vec<Mutation>>> = ...;

enum Mutation {
    CreateNode { tag: String, props: HashMap<...>, callback: Box<dyn FnOnce(u32)> },
    AppendChild { parent: u32, child: u32 },
    // ...
}

pub fn __ink_create_node(tag: &str, props_json: &str) -> u32 {
    // Parse props, queue mutation, return placeholder ID
    // Actually applied when queue is drained (after all borrows released)
}
```

### Option B: Use `RwLock` or Reentrant Mutex

Replace `RefCell` with `parking_lot::ReentrantMutex` or similar that allows recursive reads. Writes still need exclusive access.

### Option C: Make Runtime `Send + Sync` With `Arc<Mutex<...>>`

Replace thread_local RefCell with a global `Arc<Mutex<InkRuntime>>`. `Mutex` doesn't have the reentrancy problem (it deadlocks instead of panics, which is easier to debug).

## Acceptance Criteria
- [ ] Timer callbacks that call `__ink_create_node` do not panic
- [ ] Key handlers that call `__ink_create_node` do not panic
- [ ] Add test: fire timer that creates a node, verify no panic
- [ ] Miri test passes with reentrancy scenario

## Files to Modify
- `src/ink/shared.rs` — Runtime storage
- `src/ink/tree.rs` — Mutation functions
- `src/bridge/timers.rs` — Timer dispatch

## References
- RefCell docs: https://doc.rust-lang.org/std/cell/struct.RefCell.html
- Task 072 (hot reload also creates reentrancy risk)
