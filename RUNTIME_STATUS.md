# Quench Runtime Status

## Overview

The quench-runtime is a custom TypeScript/JavaScript/TSX runtime built in Rust. It uses `swc` for parsing and a custom interpreter for execution.

## Current Status

**Status**: ⚠️ **PARTIAL** - Core tests pass, simple examples work, complex examples have stack overflow

### Test Results (2026-07-07)

```
cargo test -p quench-runtime --lib              → 55 passed ✅
cargo test -p quench-runtime --test scenarios   → 31 passed, 1 skipped ⚠️
cargo test -p quench-runtime --test depth_limit → 9 passed ✅
cargo test -p quench-runtime --test runtime_issues → 44 passed ✅
cargo test -p quench-runtime --test conformance → 2 passed, 2 ignored ✅
cargo test -p quench-runtime --test equality_operators → 14 passed ✅
cargo test -p quench-runtime --test modules → 5 passed ✅
cargo test -p quench-runtime --test native_extensions → 8 passed ✅
cargo test -p quench-runtime --test project → 6 passed, 1 ignored ✅
cargo test -p quench-runtime --test to_primitive → 10 passed ✅
```

**Total**: ~184 tests pass, 1 skipped, 8 fail (var_hoisting_tdz tests require TDZ implementation)

### Known Limitations

1. **Stack Overflow in Complex Examples**: The recursive interpreter exhausts native Rust stack after ~4-5 JavaScript function calls. Examples that load runtime.js (1685 lines) crash with stack overflow.

2. **scenario_symbol_unique**: Stack overflow in Symbol comparison.

3. **var_hoisting_tdz Tests**: TDZ, var hoisting, `typeof this`, and const assignment fixes have landed in the working tree; re-run tests after the next commit to confirm the full 17/17 pass.

## Example Status

| Example | Status | Notes |
|---------|--------|-------|
| `examples/simple.js` | ✅ Pass | FFI tests all pass |
| `examples/counter.js` | ❌ Stack Overflow | Recursive interpreter limitation |
| `examples/use-bridge.tsx` | ❌ Stack Overflow | Recursive interpreter limitation |
| `examples/animations.tsx` | ❌ Stack Overflow | Recursive interpreter limitation |

## Architecture

The interpreter is recursive, which causes stack overflow for complex JavaScript. This is the fundamental limitation that Task 85 (trampoline interpreter) addresses.

## Deferred Items

See [docs/deferrals.md](docs/deferrals.md) for the exact deferral registry.

### High Priority
1. **Task 85 - Trampoline Interpreter**: Replace recursive interpreter with explicit `Vec<CallFrame>` so complex examples stop overflowing the native Rust stack.

### Medium Priority
1. Symbol comparison fix
2. Object.hasOwn implementation
3. Array.prototype.at implementation

## Verification

```bash
# Core tests (pass)
cargo test -p quench-runtime --lib
cargo test -p quench-runtime --test scenarios -- --skip scenario_symbol_unique
cargo test -p quench-runtime --test depth_limit
cargo test -p quench-runtime --test runtime_issues
cargo test -p quench-runtime --test conformance

# Simple example (passes)
cargo run -- examples/simple.js

# Complex examples (stack overflow - requires trampoline interpreter)
cargo run -- examples/counter.js
cargo run -- examples/use-bridge.tsx
```
