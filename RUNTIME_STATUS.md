# Runtime Status

## Overview

The quench-runtime is a custom TypeScript/JavaScript/TSX runtime built in Rust. It uses `swc` for parsing and a custom interpreter for execution.

## Current Status (2026-07-08)

**Status**: ⚠️ **IMPROVED** - The runtime compiles; most tests pass; examples no longer stack-overflow but still fail with hoisting/TDZ errors.

### Test Results

```
cargo test -p quench-runtime --lib              → 55 passed ✅
cargo test -p quench-runtime --test scenarios   → 39 passed ✅
cargo test -p quench-runtime --test depth_limit → 9 passed ✅
cargo test -p quench-runtime --test runtime_issues → 44 passed ✅
cargo test -p quench-runtime --test conformance → 2 passed, 2 ignored ✅
cargo test -p quench-runtime --test equality_operators → 14 passed ✅
cargo test -p quench-runtime --test modules     → 5 passed ✅
cargo test -p quench-runtime --test native_extensions → 8 passed ✅
cargo test -p quench-runtime --test project      → 6 passed, 1 ignored ✅
cargo test -p quench-runtime --test to_primitive → 10 passed ✅
cargo test -p quench-runtime --test var_hoisting_tdz → 15 passed, 2 failed ❌
cargo test -p quench-runtime --test test262      → 0 passed, 4 ignored ✅
```

**Total**: ~207 tests pass, 7 ignored, 2 fail (`var_hoisting_tdz.rs`).

### Known Limitations

1. **Hoisting / TDZ correctness**: `var_hoisting_tdz.rs` still fails 2/17 tests:
   - `test_constructor_returns_this_not_expression_value`
   - `test_tdz_shadowing_inner_let`

   These block Task 292 and must be fixed before that milestone closes.

2. **Complex examples fail on initialization errors**: `use-bridge.tsx` and `animations.tsx` fail with `ReferenceError: Cannot access 'props' before initialization`. `counter.js` logs a similar error but returns `Root node: Some(1)`.

3. **Recursive interpreter**: The interpreter is still recursive. Task 85 (trampoline interpreter) is the long-term fix for unbounded recursion.

## Example Status

| Example | Status | Notes |
|---------|--------|-------|
| `examples/simple.js` | ✅ Pass | FFI tests all pass |
| `examples/counter.js` | ⚠️ Partial | Runs but logs `ReferenceError: Cannot access 'rootId' before initialization` |
| `examples/use-bridge.tsx` | ❌ Fail | `ReferenceError: Cannot access 'props' before initialization` |
| `examples/animations.tsx` | ❌ Fail | `ReferenceError: Cannot access 'props' before initialization` |

## Architecture

The interpreter is recursive, which caused stack overflow for complex JavaScript. The depth counter prevents runaway recursion, but the proper fix is Task 85 (trampoline interpreter with explicit `Vec<CallFrame>`).

## Deferred Items

See [docs/deferrals.md](docs/deferrals.md) for the exact deferral registry.

### High Priority
1. **Task 85 - Trampoline Interpreter**: Replace recursive interpreter with explicit `Vec<CallFrame>` so complex examples stop overflowing the native Rust stack.
2. **Task 292 - var hoisting / TDZ**: Fix the remaining 2 `var_hoisting_tdz.rs` failures.

### Medium Priority
1. Object.hasOwn implementation
2. Array.prototype.at implementation

## Verification

```bash
# Core tests (pass)
cargo test -p quench-runtime --lib
cargo test -p quench-runtime --test scenarios
cargo test -p quench-runtime --test depth_limit
cargo test -p quench-runtime --test runtime_issues
cargo test -p quench-runtime --test conformance

# Hoisting/TDZ (2 known failures)
cargo test -p quench-runtime --test var_hoisting_tdz

# Simple example (passes)
cargo run -- examples/simple.js

# Complex examples (currently fail on initialization errors)
cargo run -- examples/counter.js
cargo run -- examples/use-bridge.tsx --prop theme=dark
cargo run -- examples/animations.tsx
```
