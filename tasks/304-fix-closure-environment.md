# Task 304: Fix function closure environment in call_js_function

## Status: COMPLETED

## Priority: P0 correctness

## Problem

Function declarations hoisted to global scope were not accessible inside function bodies. Tests failed with:
- "f is not defined" for hoisted functions
- "Maximum call stack size exceeded" (recursive functions couldn't find themselves)

## Root Cause

In `call_js_function`, a new empty Environment was created without linking to the function's closure:

```rust
// BROKEN: Created empty environment, lost closure
let call_env = Rc::new(RefCell::new(Environment::new()));
```

When a function body tried to access a hoisted function (like `f` in `function f() { return f(); }`), it couldn't find it because the new scope didn't inherit from the closure.

## Fix

Use `Environment::with_parent()` to properly inherit from the function's closure:

```rust
// FIXED: Inherit from closure environment
let call_env = Rc::new(RefCell::new(Environment::with_parent(Rc::clone(&func.closure))));
```

## File Changed

- `crates/quench-runtime/src/interpreter/call.rs`

## Verification

```bash
cargo test -p quench-runtime --test depth_limit
# test_recursion_depth_limit_enforced ... ok
# test_mutual_recursion_depth_limit ... ok
# test_multiple_depth_limit_hits_do_not_compound ... ok

cargo run -- examples/counter.js  # Root node: Some(1)
cargo run -- examples/use-bridge.tsx --prop theme=dark  # Root node: Some(1)
cargo run -- examples/animations.tsx  # Root node: Some(1)
```

## Test Files

- `crates/quench-runtime/tests/depth_limit.rs` - All recursion depth tests pass
