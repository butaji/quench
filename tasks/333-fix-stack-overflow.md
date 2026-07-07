> **Superseded by Task 85 (trampoline interpreter) and Task 338 (thread-local depth counter).**

# Task 333: Fix Stack Overflow in Examples

## Status: CLOSED

## Root Cause

The interpreter evaluates JavaScript by recursively calling Rust functions. Each JS call adds many Rust stack frames, so deep JS recursion exhausts the native Rust stack before any depth limit is reached. In addition, the global `CURRENT_DEPTH` counter used for runaway-recursion protection is shared across threads, so parallel tests report false stack-overflow errors.

## Exact Fix

1. **Eliminate native stack growth** — implement the trampoline interpreter described in Task 85. Replace recursive `eval_*` calls with a single `run_trampoline` loop over a heap-allocated `Vec<CallFrame>`. JS recursion must not consume the native Rust stack.
2. **Eliminate false positives in parallel tests** — implement the thread-local depth counter described in Task 338. Replace `static CURRENT_DEPTH: AtomicUsize` with a `thread_local! { static CURRENT_DEPTH: Cell<usize> }` and helper functions `check_depth()`/`release_depth()`/`reset_depth()` that read and write the thread-local cell.
3. **Keep runaway protection** — the trampoline loop checks `stack.len() >= MAX_JS_STACK` before pushing a new frame and throws a JS `RangeError: Maximum call stack size exceeded`.
4. **Do not use any of the following inferior workarounds**: increasing `RUST_MIN_STACK`, inlining functions to reduce frame count, or partial tail-call optimization only.

## Verification

```bash
timeout 60 cargo run -- examples/counter.js
timeout 60 cargo run -- examples/counter.tsx
timeout 60 cargo run -- examples/animations.tsx
cargo test -p quench-runtime
```

## Acceptance Criteria

- [ ] `counter.js` runs without stack overflow.
- [ ] `counter.tsx` runs without stack overflow.
- [ ] `animations.tsx` runs without stack overflow.
- [ ] Deeply recursive JS (e.g. `f(100000)`) runs without native stack overflow.
- [ ] All existing tests still pass.

## Targets

- **Suite:** `runtime`
- **Batch:** 1
- **Target subset:** n/a (interpreter infrastructure)
- **Blocked by:** 85, 338
- **Exit criteria:** Example apps and recursive stress tests run without stack overflow.
