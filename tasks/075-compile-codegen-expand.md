# Task 075: Expand `tests/compile_codegen.rs` to 50+ Tests

**Priority:** P1-High
**Phase:** 8 — Compile-Path Integration Tests
**Depends on:** 074

## Problem

Current `tests/compile_codegen.rs` has 23 tests that test Rust code patterns, not actual generated output from the transpiler.

## Work

1. Add tests that call `QuoteCodegen` on real TS source, then run `rustc` on the output
2. Cover all P0 features: literals, variables, binary ops, control flow, functions, arrays/objects, modules, JSX
3. Add negative tests (expected compile failures with good error messages)

## Acceptance Criteria

- [ ] `tests/compile_codegen.rs` has ≥50 tests
- [ ] Each test generates Rust from TS via `QuoteCodegen` and runs `rustc --crate-type=lib`
- [ ] All tests pass (`cargo test --test compile_codegen`)
