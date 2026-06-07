# Task 030: Delete Dead Code — runts-react, Old Scripts, HIR Runtime, Unused Imports

**Priority:** P2-Medium  
**Phase:** 3 — Cleanup  
**Status:** ✅ COMPLETED
**ETA:** 1–2 hours  
**Depends on:** 022, 028

## The Problem

Dead code accumulates: disabled `crates/runts-react/`, unused imports producing warnings, HIR runtime remnants. The 10 old `test_*.sh` parity scripts were already deleted in Task 028.

## Steps

1. Delete `crates/runts-react/` directory AND remove from workspace (`Cargo.toml`).
2. Verify `src/hir_runtime.rs` is deleted and no references remain (Task 022).
3. Remove `#[allow(dead_code)]` and fix warnings by deleting unused code.
4. Run `cargo build` and ensure zero dead-code warnings.

## Acceptance Criteria

- [x] `cargo build` passes with zero dead-code warnings.
- [x] `crates/runts-react/` directory does not exist.
- [x] `runts-react` is not listed in workspace members (`Cargo.toml`).
- [x] `scripts/` contains only `parity.sh` and `lib/`.
