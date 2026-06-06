# Task 027: Fix Compile Path — Typed Plugin Boundary + Working Binaries

**Priority:** P1-High  
**Phase:** 2 — Compile + Verification  
**ETA:** 4–6 hours  
**Depends on:** 026

## The Problem

`runts build --release` produces binaries, but:
1. Plugin boundary serializes HIR→JSON→string (lossy, slow)
2. `runts-lib` path resolution is fragile
3. No integration tests verify the binary actually works

## Steps

1. Replace `Plugin::codegen_module(hir_str: &str)` with typed `&hir::Module`.
2. Fix `find_runts_lib_path` to use `env!("CARGO_MANIFEST_DIR")`.
3. Add `tests/compile_path.rs` with tests for 5 static examples.
4. Run `./scripts/parity.sh --env compile` and fix failures.

## Acceptance Criteria

- [ ] `runts build --release --plugin ratatui` on static examples produces working binary.
- [ ] No JSON round-trip in plugin boundary.
- [ ] Integration tests exist for compile path.
