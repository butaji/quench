# Task 039: Add Compile-Path Integration Tests That Actually Run `cargo build`

**Priority:** P1-High
**Phase:** 5 — Compile Path Hardening
**Depends on:** 038

## Problem

The 864 passing tests in `src/transpile/tests/` were almost entirely HIR-builder tests. Only ~5 tests actually ran `cargo build` on generated Rust.

## Acceptance Criteria

- [x] `tests/compile_codegen.rs` exists with >=5 tests that run `rustc --crate-type=lib` on generated Rust
- [x] Each test covers a distinct codegen construct
- [x] All tests pass (`cargo test --test compile_codegen`)
- [x] Test results added to `tasks/index.json` stats
