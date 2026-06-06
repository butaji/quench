# Task 029: Add Per-Example Unit Tests for All 88 Examples

**Priority:** P2-Medium  
**Phase:** 2 — Compile + Verification  
**ETA:** 4–6 hours  
**Depends on:** 028

## The Problem

Only ~15 examples have inline tests. 74 have zero coverage.

## Steps

1. Create `tests/rq_parity/mod.rs` with a test generator macro.
2. Generate one test per example:
   ```rust
   ink_example_test!(test_ink_text_props, "examples/ink-text-props/tui/app.tsx", &["HIGHLIGHTED"]);
   ```
3. Run `cargo test --test rq_parity` and fix failures.
4. Add CI check that every example file has a matching test.

## Acceptance Criteria

- [ ] 88 tests, one per example.
- [ ] `cargo test` passes.
- [ ] Coverage >= 90%.
