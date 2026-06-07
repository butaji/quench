# Task 076: Add Compile-Path Parity Tests for All New Examples

**Priority:** P1-High
**Phase:** 8 — Compile-Path Integration Tests
**Depends on:** 075

## Problem

The parity harness supports `--env compile` but most new examples won't compile until their codegen is fixed.

## Work

1. Extend `scripts/parity.sh` to gracefully handle compile-path failures
2. As each example task (042–067) is completed, verify it passes in the compile environment
3. Track compile-path pass rate in `tasks/index.json`

## Acceptance Criteria

- [ ] Parity harness can run `--env compile` for all examples
- [ ] Compile-path pass rate tracked in stats
- [ ] Each example that passes compile path is documented
