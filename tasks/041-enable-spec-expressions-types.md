# Task 041: Enable `spec_expressions` and `spec_types` Test Modules

**Priority:** P1-High  
**Phase:** 6 — Test Infrastructure  
**Depends on:** 040

## Problem

Two test directories exist but are **not wired into** `src/transpile/tests/mod.rs`:

- `src/transpile/tests/spec_expressions/` — 10 test files (arithmetic, bitwise, comparison, logical, member/call, ternary, templates, unary, complex)
- `src/transpile/tests/spec_types/` — 7 test files (primitive types, complex types, type declarations, type-directed lowering, codegen verification, roundtrip)

## Work

1. Add `#[cfg(test)] pub mod spec_expressions;` to `src/transpile/tests/mod.rs`
2. Add `#[cfg(test)] pub mod spec_types;` to `src/transpile/tests/mod.rs`
3. Run `cargo test --bin runts`
4. Fix compilation errors (helper visibility, imports)
5. For failing tests: fix underlying bug or add `#[ignore = "reason"]` with explanation

## Acceptance Criteria

- [ ] Both modules compile and run with `cargo test --bin runts`
- [ ] `cargo test --bin runts` exits 0 (passing + documented ignored tests only)
- [ ] Every ignored test has a documented reason
