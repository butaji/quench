# Task 022: DELETE src/hir_runtime.rs and All HIR Interpreter Code

**Priority:** P0-Critical  
**Phase:** 1 — rquickjs Dev Engine  
**Status:** ✅ COMPLETED
**ETA:** 1 hour  
**Depends on:** 020, 021

## The Problem

`src/hir_runtime.rs` is 3,087 lines of a custom JavaScript engine: expression evaluator, JSX mapper, CSS prop applier, hook polyfills, string/array polyfills, color parser, and 70 inline tests. This code path is dead weight. We are using rquickjs instead.

## Steps

1. Delete `src/hir_runtime.rs`.
2. Remove `mod hir_runtime;` from `src/lib.rs` or `src/main.rs`.
3. Remove `run_hir_render` and `run_inspect_hir` from `src/main.rs`.
4. Remove `cli::Commands::HirRender` and `cli::Commands::InspectHir` from `src/cli.rs`.
5. Remove `hir_runtime` tests from `src/hir_runtime.rs` (already deleted).
6. Search and remove all `Interpreter`, `Value`, `render_tsx`, `RuntimeError` references across the codebase.
7. Run `cargo build` and fix any compile errors.

## Acceptance Criteria

- [x] `src/hir_runtime.rs` does not exist.
- [x] `cargo build` passes.
- [x] No references to HIR interpreter in source code.
- [x] `runts` CLI no longer has `hir-render` or `inspect-hir` commands.
