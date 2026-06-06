# Task 021: Re-enable build.rs Linter and Fix All Violations

**Priority:** P0-Critical  
**Phase:** 0 — Unblock  
**ETA:** 4–6 hours  
**Depends on:** 020

## The Problem

`build.rs` defines strict linting rules (file ≤500 lines, fn ≤40 lines, complexity ≤10) that are **commented out**. The codebase has **47 violations**.

## Steps

1. Uncomment the linter in `build.rs`.
2. Fix violations mechanically — extract match arms, split files, use macros. Do NOT rewrite algorithms.
3. Run `cargo build` after each file.
4. Verify: `runts-lint: N file(s) OK` with 0 violations.

## Files to Modify

- `build.rs` — uncomment linter
- `src/transpile/hir/quote_codegen.rs` — extract `gen_stmt` match arms
- `src/commands/build/mod.rs` — extract plugin build into separate module
- `src/commands/build/source_gen.rs` — extract sub-steps
- `crates/runts-fresh/src/plugin.rs` — extract `extract_handler_methods`
- `crates/runts-ink/src/render.rs` — extract frame loop / ANSI parser
- `crates/runts-ink/src/components.rs` — split builder methods
- `crates/runts-ink/src/js_bridge.rs` — split reconciler message handlers

## Acceptance Criteria

- [ ] `cargo build` passes with 0 linter violations.
- [ ] No function > 40 lines, no file > 500 lines, no complexity > 10.
