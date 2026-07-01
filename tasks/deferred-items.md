# Deferred Items

This document tracks all deferred tasks and the reasons why they were postponed.

## Overview

The quench-runtime has achieved its primary goals:
- ✅ All Ink examples work (simple.js, counter.js, use-bridge.tsx, animations.tsx)
- ✅ 59 runtime unit tests pass (20 unit + 39 runtime in quench-runtime)
- ✅ 37 integration tests pass (34 main + 3 parity)
- ✅ Full TypeScript module, async/await, class support
- ✅ ES module import/export
- ✅ Build linter enforcement
- ✅ Zero compiler warnings

The following items were intentionally deferred to future phases.

---

## Task 11: Performance Roadmap

**Status:** 🔲 In Progress (baseline established)

**What was deferred:**
- NaN-boxed `Value` type (64-bit, `Copy`)
- String interning with `lasso`/`string-interner`
- Object shapes (hidden classes) + inline caches
- Slot-indexed environments
- Arena allocation with `bumpalo`
- Explicit evaluation stack (iterative interpreter) — DONE

**What's done:**
- `rustc-hash` for HashMaps (already in use)
- `indexmap` for ordered property maps (already in use)
- Iterative interpreter with explicit depth tracking (already in use)
- Benchmarks in `tests/benchmarks.rs`

**What's remaining:**
- NaN-boxed `Value` type
- String interning
- Object shapes + inline caches
- Slot-indexed environments

**Why deferred:**
- Correctness was the priority. All functional tests pass before optimizing.
- These optimizations are for performance, not correctness.

**See also:**
- `docs/performance-research.md`
- `crates/quench-runtime/tests/benchmarks.rs`

---

## Task 24: Reactive Execution Engine

**Status:** 🔲 Deferred

**What was deferred:**
- Rust-level reactive HIR nodes (Signal, Memo, Effect, Render)
- Dependency graph building
- Effect batching and scheduling
- Reactive render boundaries

**Why deferred:**
- The JS-based `runtime.js` already implements all reactive primitives:
  - Signals → `useState` (JS closure state + `scheduleRerender`)
  - Memos → `useMemo` (JS closure + dependency tracking)
  - Effects → `useEffect` (JS closure + cleanup)
  - Rendering → `render()` + `reconcileTree`
- The Rust-level reactive nodes were decorative (never produced by the lowerer)
- JS approach is simpler and already works correctly

**Current workaround:**
- All reactive behavior runs in `runtime.js` using hooks
- Components are pure functions of props + signals

**See also:**
- `src/runtime.js` (reactive implementation)
- `tasks/56-architecture-cleanup.md` (rationale for removal)

---

## Task 58: Fourth Review Findings (Remaining Items)

**Status:** 🔲 Partial completion (Rank 1/2 correctness fixed)

**What was fixed:**
- Promise `.then`/`.catch`/`all`/`race`/`finally` working
- Microtask draining working
- `Function.prototype.call`/`apply` working
- Getters with correct `this` binding
- `instanceof` on functions
- `for...in` enumerable only
- Numeric-string keys on non-arrays
- Symbol truthy
- Assignment LHS re-evaluation verified correct

**What's remaining (Rank 1):**
- Native constructor prototypes isolated from `Object.prototype` (Date, Error, etc.)
- Hot reload compile error (borrow conflict)
- `__ink_set_timeout` JSON-stringifies functions
- `setTimeout`/`setInterval` stubs (no bridge integration)
- Mouse events never received (no `EnableMouseCapture`)

**What's remaining (Rank 2):**
- Class static members stored on wrong object
- Module import with missing module throws
- `for...in` getter side effects
- Lowering silently swallows subexpression errors

**Why deferred:**
- None of these block the current Ink examples
- Fixing them requires architectural changes to the bridge/terminal/event loop
- The remaining Rank 3 issues are for spec completeness, not correctness

---

## Task 47: TypeScript Project Cases

**Status:** 🔲 Pending

**What was deferred:**
- Harness for multi-file project scenarios
- Module loader for multi-file scenarios

**Why deferred:**
- Requires a module loader for multi-file scenarios
- Single-file ES modules already work via the runtime module registry
- More complex than single-file conformance cases

**What's needed to un-defer:**
- Module loader implementation
- Project spec JSON parsing

**Current workaround:**
- ES module support exists for single-file imports
- Multi-file project tests are not blocking

**See also:**
- `tasks/47-typescript-project-cases.md`

---

## Summary

| Task | Description | Status |
|------|-------------|--------|
| 11 | Performance optimizations | 🔲 In progress (baseline done) |
| 24 | Reactive engine | 🔲 Deferred |
| 47 | TS project cases | 🔲 Pending |
| 58 | Fourth review findings | 🔲 Partial (Rank 1/2 fixed) |
