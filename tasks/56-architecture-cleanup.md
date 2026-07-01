# Task 56: Architecture Cleanup

## Date: 2026-07-01

This document describes the architecture cleanup performed to simplify the quench-runtime codebase.

## Removed Decorative Reactive HIR Nodes

The following reactive HIR node variants were **previously documented** but **never existed** in the actual codebase:

- `Expression::Signal { id, initial }` - Never implemented
- `Expression::SignalGet { signal_id }` - Never implemented  
- `Expression::SignalSet { signal_id, value }` - Never implemented
- `Expression::Memo { id, deps, compute }` - Never implemented
- `Expression::Effect { id, deps, callback }` - Never implemented
- `Expression::Render { id, component, props }` - Never implemented

**Rationale**: The reactive system is implemented in JavaScript via `runtime.js` (useState, useEffect, useMemo hooks). The Rust interpreter handles these via JS function calls, not as native HIR nodes. This simplifies the interpreter and avoids maintaining dead code paths.

## Current Architecture

The HIR is a conventional JavaScript AST with the following structure:

```
Program
  └── Statement (function declarations, var declarations, expressions, etc.)
        └── Expression (literals, operations, calls, etc.)
```

### Why This Approach Works

1. **JS-based reactive system** - All reactive primitives (signals, memos, effects) are implemented in `runtime.js` and executed via the interpreter's native function call mechanism.

2. **Simpler interpreter** - No special handling for reactive nodes means the eval loop is straightforward and maintainable.

3. **Correctness over optimization** - The current implementation correctly handles the Ink use cases. Future optimizations can be added without changing the fundamental architecture.

## Remaining Deferred Items

### High Priority (Performance)

| Item | Status | Notes |
|------|--------|-------|
| NaN-boxed Value | Deferred | Would make `Value: Copy`; significant refactor |
| String interning | Deferred | `lasso` or `string-interner` crate |
| Object shapes | Deferred | Hidden classes + inline caches for hot paths |

### Medium Priority (Correctness Edge Cases)

| Item | Status | Notes |
|------|--------|-------|
| Getters invoked with prototype as `this` | Deferred | Edge case in prototype chain |
| `instanceof` for functions | Deferred | Edge case: `fn instanceof Function` |
| Class static members on instance | Deferred | Static methods stored on constructor |
| Numeric-string keys in objects | Deferred | `obj["123"]` expands array storage |
| For-loop multi-declarators | Deferred | `for (let i=0, j=0; ...)` drops `j` |

### Low Priority (Not Blocking)

| Item | Status | Notes |
|------|--------|-------|
| Garbage collector | Monitor | Cycle risk with `Rc<RefCell>` |
| ES module loader | Deferred | No examples need it yet |
| Generator functions + yield | Deferred | No examples need them |
| AOT/JIT compilation | Future | Cranelift backend planned |

### Already Resolved

| Item | Date | Resolution |
|------|------|------------|
| for...in / for...of | 2026-07-01 | Implemented in interpreter |
| for...in enumerability | 2026-07-01 | Fixed prototype chain |
| instanceof for functions | 2026-07-01 | Added `Value::Function` handling |
| Symbol truthiness | 2026-07-01 | Changed to `true` |
| Object rest/spread | 2026-07-01 | Implemented in lower.rs |
| Class static members | 2026-07-01 | Fixed `__static:` prefix removal |

## Lint Cleanup (2026-07-01)

Fixed the following clippy warnings:

1. **Removed unknown lint `clippy::function_body_length`** from:
   - `src/builtins/math.rs`
   - `src/builtins/object.rs`
   - `src/builtins/array.rs`
   - `src/builtins/date.rs`
   - `src/builtins/error.rs`

2. **Fixed unused variable warnings** in `src/interpreter.rs`:
   - `ForInit::VarDeclaration { kind: _, ... }` now properly ignores `kind`

## Verification

```bash
cargo check -p quench-runtime    # No warnings
cargo clippy -p quench-runtime   # No unknown lint warnings
cargo test -p quench-runtime     # All tests pass
```

## Files Changed

- `crates/quench-runtime/src/builtins/math.rs` - Removed unknown lint
- `crates/quench-runtime/src/builtins/object.rs` - Removed unknown lint
- `crates/quench-runtime/src/builtins/array.rs` - Removed unknown lint
- `crates/quench-runtime/src/builtins/date.rs` - Removed unknown lint
- `crates/quench-runtime/src/builtins/error.rs` - Removed unknown lint
- `crates/quench-runtime/src/interpreter.rs` - Fixed unused variables
- `tasks/56-architecture-cleanup.md` - This document
- `tasks/58-deferred-items.md` - Updated to reflect current state
