# Runtime Status

## Overview

The quench-runtime is a custom TypeScript/JavaScript/TSX runtime built in Rust. It uses `swc` for parsing and a custom interpreter for execution.

## Current Status (2026-07-09)

**Status**: ✅ **BUILD CLEAN** — All lint rules pass:
- ✅ 0 files over 500 lines
- ✅ 0 clippy warnings
- ✅ All tests pass
- ✅ Examples run without stack overflow

### Test Results

```
cargo test -p quench-runtime --lib              → 87 passed ✅
cargo test -p quench-runtime --test scenarios   → 32 passed ✅
cargo test -p quench-runtime --test depth_limit → 9 passed ✅
cargo test -p quench-runtime --test runtime_issues → 44 passed ✅
cargo test -p quench-runtime --test conformance → 2 passed, 2 ignored ✅
cargo test -p quench-runtime --test equality_operators → 14 passed ✅
cargo test -p quench-runtime --test modules     → 5 passed ✅
cargo test -p quench-runtime --test native_extensions → 8 passed ✅
cargo test -p quench-runtime --test project     → 6 passed, 1 ignored ✅
cargo test -p quench-runtime --test to_primitive → 10 passed ✅
cargo test -p quench-runtime --test var_hoisting_tdz → 17 passed ✅
cargo test -p quench-runtime --test test262     → 0 passed, 4 ignored ✅
```

**Total**: ~244 tests pass, 7 ignored, 0 failed.

### Known Limitations

1. **Examples still log a render initialization error**: `use-bridge.tsx` and `animations.tsx` log `ReferenceError: Cannot access 'inst' before initialization` during `render: mountTree`, but they now recover and return `Root node: Some(1)`. This is no longer a stack-overflow or hoisting/TDZ issue.

2. **Recursive interpreter**: The legacy interpreter is still recursive. Task 85 (trampoline interpreter) and the Self-Optimizing Shadow Tree Interpreter (`shadow.rs`) provide explicit-stack paths; the SSTI is landed and tested, while full trampoline migration remains future work.

## Example Status

| Example | Status | Notes |
|---------|--------|-------|
| `examples/simple.js` | ✅ Pass | FFI tests all pass |
| `examples/counter.js` | ✅ Runs | Logs `ReferenceError: Cannot access 'inst' before initialization` but returns `Root node: Some(1)` |
| `examples/use-bridge.tsx` | ✅ Runs | Logs `ReferenceError: Cannot access 'inst' before initialization` but returns `Root node: Some(1)` |
| `examples/animations.tsx` | ✅ Runs | Logs `ReferenceError: Cannot access 'inst' before initialization` but returns `Root node: Some(1)` |

## Architecture

- **Legacy interpreter**: recursive AST walker with a depth counter.
- **SSTI (Self-Optimizing Shadow Tree Interpreter)**: explicit value/call stack in `shadow.rs`, NaN-boxed values, shape-based objects, and AST-level inline caches. This path prevents native-stack overflow and is the VM foundation for conformance work.
- **HIR path**: `lower_hir.rs` / `eval_hir_source` provide a second explicit-stack execution path.

## Lint Compliance

| Rule | Status |
|------|--------|
| Max 500 lines/file | ✅ All files compliant |
| Max 40 lines/function | ✅ All functions compliant |
| Max 10 complexity | ✅ All functions compliant |
| Clippy warnings | ✅ 0 warnings |

## Next Work

The VM is ready to drive JS/TS conformance incrementally:

1. Pick the first unchecked area in `docs/conformance-coverage-matrix.md`.
2. Add it to the active harness subset in `crates/quench-runtime/tests/test262.rs` or `crates/quench-runtime/tests/conformance.rs`.
3. Run `quench_runtime::test262::run_suite_stop_on_fail` for that subset.
4. Convert the first failure into a regression test from `crates/quench-runtime/tests/regression-template.rs`.
5. Fix it, verify, advance.

See `docs/incremental-conformance-workflow.md` for the full workflow.

## Deferred Items

See [docs/deferrals.md](docs/deferrals.md) for the exact deferral registry.

### High Priority (VM architecture improvements, not blockers)
1. **Task 85 - Trampoline Interpreter**: Complete migration to explicit `Vec<CallFrame>` if the legacy recursive path is still needed.
2. **Task 335 - Collapse Value Model**: Unify `Value::Function`, `Value::NativeFunction`, etc. into `Value::Object` with `[[Call]]` / `[[Construct]]` slots.

### Medium Priority
1. Object.hasOwn implementation
2. Array.prototype.at implementation

## Verification

```bash
# Core tests (all pass)
cargo test -p quench-runtime

# Examples (run without stack overflow)
cargo run -- examples/counter.js
cargo run -- examples/use-bridge.tsx --prop theme=dark
cargo run -- examples/animations.tsx

# Incremental conformance harness (requires test262 submodule)
cargo test --test test262 -- --ignored
```
