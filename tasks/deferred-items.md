# Deferred Items

This document tracks all deferred tasks and the reasons why they were postponed.

## Overview

The quench-runtime has achieved its primary goals:
- ✅ All Ink examples work (simple.js, counter.js, use-bridge.tsx, animations.tsx)
- ✅ 103 runtime unit tests pass
- ✅ 97.4% source-direct TypeScript conformance
- ✅ Full TypeScript module, async/await, class support
- ✅ ES module import/export
- ✅ Build linter enforcement

The following items were intentionally deferred to future phases.

---

## Performance Optimizations (Task 11)

**Status:** 🔲 In Progress

**What was deferred:**
- NaN-boxed `Value` type (64-bit, `Copy`)
- String interning with `lasso`/`string-interner`
- Object shapes (hidden classes) + inline caches
- Slot-indexed environments
- Arena allocation with `bumpalo`
- Explicit evaluation stack (iterative interpreter)

**Why deferred:**
- Correctness was the priority. All functional tests pass before optimizing.
- These optimizations are for performance, not correctness.
- The interpreter is fast enough for current Ink use cases.

**What's needed to un-defer:**
- Benchmark infrastructure to measure baseline performance
- Acceptance criteria for performance improvements

**Current workaround:**
- The interpreter uses `rustc-hash` (already fast) for HashMaps
- `indexmap` for ordered property maps
- Simple microbenchmarks in `tests/benchmarks.rs`

**See also:**
- `docs/performance-research.md`
- `crates/quench-runtime/tests/benchmarks.rs`

---

## Reactive Execution Engine (Task 24)

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

**What's needed to un-defer:**
- Performance evidence that Rust-level reactivity is necessary
- A concrete use case that JS-level reactivity cannot handle

**Current workaround:**
- All reactive behavior runs in `runtime.js` using hooks
- Components are pure functions of props + signals

**See also:**
- `src/runtime.js` (reactive implementation)
- `tasks/56-architecture-cleanup.md` (rationale for removal)

---

## TypeScript Runner Reference (Task 30)

**Status:** 🔲 Optional / Reference Only

**What was deferred:**
- Running TypeScript's own test suite as a reference
- Generating baselines for comparison

**Why deferred:**
- This is for validation, not required for development
- Quench has its own conformance harness that provides sufficient coverage

**What's needed to un-defer:**
- CI integration for periodic baseline comparisons
- Documentation of how to interpret differences

**Current workaround:**
- Quench's own `conformance.rs` harness runs TypeScript conformance tests
- `scripts/run_typescript_runner.sh` provides a script for running TS tests

**See also:**
- `docs/typescript-tests.md`
- `tasks/35-typescript-compiler-runner.md`

---

## TypeScript Compiler Runner (Task 35)

**Status:** 🔲 New Harness Created

**What was deferred:**
- Harness for ~6500 TypeScript compiler regression cases

**Why deferred:**
- The conformance harness covers the runtime-relevant subset
- Compiler cases are mostly type-check focused

**What's needed to un-defer:**
- `crates/quench-runtime/tests/compiler_cases.rs` (new harness created)

**Current workaround:**
- New `compiler_cases.rs` harness runs compiler cases directly
- Skips type-check only files via `@noEmit` directive detection

**See also:**
- `crates/quench-runtime/tests/compiler_cases.rs`
- `tasks/46-typescript-compiler-cases.md`

---

## Conformance Validation (Task 43)

**Status:** 🔲 Pending Integration

**What was deferred:**
- Using TypeScript's own runner to validate Quench results

**Why deferred:**
- Reference validation is for confidence, not blocking

**What's needed to un-defer:**
- Setup instructions in CI
- Baseline comparison tooling

**Current workaround:**
- `docs/typescript-tests.md` documents how to run TS tests manually
- `scripts/run_typescript_runner.sh` automates the process

**See also:**
- `docs/typescript-tests.md`

---

## TypeScript Evaluation Tests (Task 45)

**Status:** 🔲 Harness Created

**What was deferred:**
- Porting TypeScript evaluation unit tests to Quench

**What's been done:**
- `crates/quench-runtime/tests/evaluation.rs` created
- Discovers and runs evaluation tests from `tests/typescript/src/testRunner/unittests/evaluation/`

**Current status:**
- Harness exists and runs tests
- Parse errors are skipped (not failed)
- Runtime errors are counted as failures

**See also:**
- `crates/quench-runtime/tests/evaluation.rs`

---

## TypeScript Compiler Cases (Task 46)

**Status:** 🔲 Harness Created

**What was deferred:**
- Harness for TypeScript compiler regression cases

**What's been done:**
- `crates/quench-runtime/tests/compiler_cases.rs` created
- Discovers ~6500 compiler cases from `tests/typescript/tests/cases/compiler/`
- Parses directives (@target, @module, @noEmit, etc.)
- Runs files directly in quench-runtime

**Current status:**
- Harness exists with sample tests
- Type-check only files are skipped

**See also:**
- `crates/quench-runtime/tests/compiler_cases.rs`

---

## TypeScript Project Cases (Task 47)

**Status:** 🔲 Pending

**What was deferred:**
- Harness for multi-file project scenarios

**Why deferred:**
- Requires a module loader for multi-file scenarios
- More complex than single-file conformance cases

**What's needed to un-defer:**
- Module loader implementation (if not already present)
- Project spec JSON parsing

**Current workaround:**
- ES module support exists for single-file imports
- Multi-file project tests are not blocking

**See also:**
- `tasks/47-typescript-project-cases.md`

---

## Reference Runners (Task 48)

**Status:** 🔲 Optional

**What was deferred:**
- Using `hereby runtests --runner=compiler,conformance,project,transpile`

**Why deferred:**
- CI complexity vs. value
- Quench has its own harness

**What's needed to un-defer:**
- CI pipeline setup
- Baseline comparison workflow

**See also:**
- `docs/typescript-tests.md`

---

## CI Commands (Task 49)

**Status:** ✅ Created

**What's been done:**
- `scripts/run_typescript_runner.sh` created and made executable
- Covers conformance, compiler, and project runners

**See also:**
- `scripts/run_typescript_runner.sh`

---

## Test Documentation (Task 50)

**Status:** ✅ Created

**What's been done:**
- `docs/typescript-tests.md` created with full documentation

**See also:**
- `docs/typescript-tests.md`

---

## Summary

| Task | Description | Status |
|------|-------------|--------|
| 11 | Performance optimizations | 🔲 In progress |
| 24 | Reactive engine | 🔲 Deferred |
| 30 | TS runner reference | 🔲 Optional |
| 35 | TS compiler runner | 🔲 Harness exists |
| 43 | Conformance validation | 🔲 Pending |
| 45 | TS evaluation tests | ✅ Harness created |
| 46 | TS compiler cases | ✅ Harness created |
| 47 | TS project cases | 🔲 Pending |
| 48 | Reference runners | 🔲 Optional |
| 49 | CI commands | ✅ Done |
| 50 | Test docs | ✅ Done |
