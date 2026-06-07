# Task 081: Expand `tests/compile_codegen.rs` to 50+ Tests

**Priority:** P1-High
**Phase:** 8 — Compile-Path Integration Tests
**Depends on:** 079

## Problem

Current `tests/compile_codegen.rs` has 23 tests, but they test Rust code patterns, not actual generated output from the transpiler.

## Work

1. Add tests that actually call `QuoteCodegen` on real TS source, then run `rustc` on the output
2. Cover all P0 features:
   - Literals (string, number, bool, null, undefined, bigint, regexp)
   - Variables (let, const, var)
   - Binary ops (+, -, *, /, %, ==, !=, <, >, <=, >=)
   - Control flow (if, while, for, switch, try/catch)
   - Functions (declarations, arrows, calls, async)
   - Arrays/Objects (literals, member access, spread)
   - Modules (import, export)
   - JSX (elements, components, fragments, attrs)
3. Add negative tests (expected compile failures with good error messages)

## Acceptance Criteria

- [ ] `tests/compile_codegen.rs` has ≥50 tests
- [ ] Each test generates Rust from TS source via `QuoteCodegen`
- [ ] Each test runs `rustc --crate-type=lib` on generated output
- [ ] All tests pass (`cargo test --test compile_codegen`)
