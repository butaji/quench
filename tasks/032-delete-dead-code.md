# Task 032: Delete Dead Code: runts-react Crate, Disabled Imports, Unused Test Script Graveyard

**Priority:** P2-Medium  
**Phase:** 4 — Verification & Hardening  
**ETA:** 1 hour  
**Depends on:** 020

## The Problem

- `crates/runts-react/` is in the workspace but commented out in root `Cargo.toml`.
- `test_*.sh` graveyard still exists after Task 023.
- Multiple `#[allow(dead_code)]` annotations hide unused functions.

## Steps

1. Remove `crates/runts-react/` entirely, or remove it from workspace members if keeping for later.
2. After Task 023 is complete, delete all old `test_parity*.sh`, `test_ink_parity*.sh`, `run_parity_tests*.sh` scripts.
3. Run `cargo build` and fix any `dead_code` warnings by deleting unused code, not by adding `#[allow]`.

## Acceptance Criteria

- [ ] `cargo build` passes with no dead-code warnings.
- [ ] Workspace members only include compiling crates.
- [ ] Only `scripts/parity.sh` remains for parity testing.
