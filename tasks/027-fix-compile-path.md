# Task 027: Fix Compile Path — Typed Plugin Boundary + Working Binaries

**Priority:** P1-High  
**Phase:** 2 — Compile + Verification  
**Status:** ✅ COMPLETED
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

## Notes

- The plugin boundary IS typed (`Plugin::codegen_module(&runts_hir::Module)`), but `runts-ratatui/src/plugin.rs` still does an internal JSON round-trip (`serde_json::to_value(&module.items)`) for the existing JSX codegen. This is acceptable for now — the boundary fix is done.
- `find_runts_lib_path` was replaced by `find_runts_ink_path` using `env!("CARGO_MANIFEST_DIR")`.

## Acceptance Criteria

- [x] `runts build --release --plugin ratatui` on static examples produces working binary.
- [x] Typed plugin boundary (`&runts_hir::Module` instead of `&str`).
- [x] Integration tests exist for compile path (`tests/compile_path.rs`).
