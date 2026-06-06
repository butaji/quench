# Task 030: Delete Dead Code — runts-react, Old Scripts, HIR Runtime, Unused Imports

**Priority:** P2-Medium  
**Phase:** 3 — Cleanup  
**ETA:** 1–2 hours  
**Depends on:** 022, 028

## The Problem

Dead code accumulates: disabled crates, 20 old scripts, HIR runtime remnants, unused imports.

## Steps

1. Delete `crates/runts-react/` or remove from workspace.
2. Delete all old parity scripts.
3. Delete `src/hir_runtime.rs` and all references (Task 022).
4. Remove `#[allow(dead_code)]` and fix warnings by deleting unused code.
5. Run `cargo build` and ensure zero warnings.

## Acceptance Criteria

- [ ] `cargo build` passes with zero dead-code warnings.
- [ ] Workspace members only include compiling crates.
- [ ] `scripts/` contains only `parity.sh` and `lib/`.
