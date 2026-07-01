# Task 56: Architecture Cleanup

## Goal

Clean up decorative/unused code from the quench-runtime crate and fix lint warnings. Document architectural items that are intentionally deferred.

## TDD note

Follow the red-green-refactor cycle. Add a failing unit test first, then the minimal code to pass it.

---

## 1. Removed Decorative Reactive HIR Nodes

The following variants existed in `Expression` in `crates/quench-runtime/src/ast.rs` but were **never produced by the lowerer** and only ever caused `"must be lowered/handled by the reactive engine (Task 24)"` errors at runtime:

| Variant | Description | Why removed |
|---------|-------------|-------------|
| `Signal { id, initial }` | Reactive signal declaration | Never produced by lowerer |
| `SignalGet { signal_id }` | Reactive signal read | Never produced by lowerer |
| `SignalSet { signal_id, value }` | Reactive signal write | Never produced by lowerer |
| `Memo { id, deps, compute }` | Reactive memoized computation | Never produced by lowerer |
| `Effect { id, deps, callback }` | Reactive side effect | Never produced by lowerer |
| `Render { id, component, props }` | Reactive component render boundary | Never produced by lowerer |

### Rationale

The JS-based `runtime.js` already implements all reactive primitives through hooks:

- **Signals** → `useState` (JS closure state + `scheduleRerender`)
- **Memos** → `useMemo` (JS closure + dependency tracking)
- **Effects** → `useEffect` (JS closure + cleanup)
- **Rendering** → `render()` + `ComponentInstance` + `reconcileTree`

These Rust-level nodes were added in Task 22 for a planned reactive engine (Task 24), but the JS approach works correctly and is simpler. Maintaining Rust-level reactive nodes that the lowerer never produces, and that the interpreter never executes, is confusing and creates dead error paths.

### Files changed

- `crates/quench-runtime/src/ast.rs` — removed 6 enum variants
- `crates/quench-runtime/src/interpreter/eval_expr/main.rs` — removed 6 error stubs
- `crates/quench-runtime/src/ast.rs` — added `test_no_reactive_nodes_in_expression` regression test

---

## 2. Fixed Lint Warnings

### Warning: unused `chrono::Utc` import

**File:** `crates/quench-runtime/tests/conformance.rs` line 19
**Fix:** Removed `use chrono::Utc;`
**Rationale:** The import was added for date handling but never used; `Date` builtins use Rust's `std::time` instead.

### Warning: non-snake_case function names

**File:** `crates/quench-runtime/tests/runtime_tests.rs`

| Old name | New name |
|----------|----------|
| `test_globalThis_has_globals` | `test_global_this_has_globals` |
| `test_globalThis_methods_work` | `test_global_this_methods_work` |

---

## 3. Deferred Architectural Items

The following items are intentionally deferred. They are documented here so they are not forgotten.

### Performance Optimizations (Task 11)

These are documented in `tasks/11-performance.md`. All are **deferred** until the runtime is functionally correct:

| Item | Status | Rationale |
|------|--------|-----------|
| NaN-boxed `Value` representation | Deferred | Makes `Value: Copy` and 64-bit; avoids heap allocation for primitives. Currently uses `Rc<RefCell>`. Significant refactor; correct first. |
| String interning (`lasso` / `string-interner`) | Deferred | Converts identifiers/property names to `u32` atoms. `HashMap<Atom, Value>` faster. Not needed for correctness. |
| Object shapes + inline caches | Deferred | Hidden classes + ICs at hot AST nodes. Major HIR/eval refactor. Not needed for correctness. |
| Slot-indexed environments | Deferred | Scope analysis assigns stack slots; locals stored in `Vec<Value>`. Removes `HashMap` lookups for variables. |
| Arena allocation (`bumpalo`) | Deferred | Arena for frames, temp objects. Improves allocation performance. |
| Faster maps (`rustc-hash` / `foldhash`) | Deferred | Integer-atom-keyed maps. Not needed for correctness. |

### Language Features (Task 19)

| Item | Status | Rationale |
|------|--------|-----------|
| ES module loader (`import`/`export`) | Deferred | No Ink examples use external module imports. Lowerer already handles module syntax; just returns `None`. |
| Generator functions + `yield` | Deferred | No examples need generators. Would need a CPS transform or trampoline interpreter. |
| `Symbol.iterator` explicit implementation | Deferred | `for...of` already works via `ObjectKind` detection for Map/Set/Array. Explicit `Symbol.iterator` not needed yet. |

### Garbage Collector

| Item | Status | Rationale |
|------|--------|-----------|
| GC / cycle detection | Monitor | Values use `Rc<RefCell>`. JS code doesn't create obvious reference cycles. Monitor for leaks; add `Rc::downgrade` + weak refs or a tracing GC later if needed. |

### AOT / JIT Compilation (Task 24, Task 25)

| Item | Status | Rationale |
|------|--------|-----------|
| Reactive execution engine (Rust-level) | Deferred | `runtime.js` hooks cover all reactive needs. No Rust-level reactive graph needed. |
| Cranelift AOT/JIT backend | Deferred | Interpreter is fast enough for Ink apps. Would need HIR-to-Cranelift lowering. Not in scope. |
| Bytecode VM | Deferred | Interpreter covers all current use cases. A bytecode layer would delay getting Ink apps working. |

### Diagnostic Improvements (Task 23)

| Item | Status | Rationale |
|------|--------|-----------|
| `miette` / `ariadne` pretty-print | Deferred | `JsError` has source locations; full pretty-print with snippets is nice-to-have. |
| Runtime stack traces | Deferred | Currently only error type/name. Full JS stack traces require capturing eval frames. |

---

## Verification

```bash
# Runtime tests pass
timeout 120 cargo test -p quench-runtime

# Full test suite passes
timeout 120 cargo test

# Build produces no warnings
timeout 30 cargo build 2>&1
```

---

## Status: COMPLETED

- ✅ Reactive HIR nodes removed from `ast.rs` and interpreter
- ✅ Unit test added: `test_no_reactive_nodes_in_expression`
- ✅ `chrono::Utc` unused import removed from `conformance.rs`
- ✅ `test_globalThis_has_globals` renamed to `test_global_this_has_globals`
- ✅ `test_globalThis_methods_work` renamed to `test_global_this_methods_work`
- ✅ Deferred work documented in this file
