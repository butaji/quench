# Task 360: 100% JS/TS Compatibility Push

## Status: IN PROGRESS

## Goal
Reach **100% compatibility with JavaScript, TypeScript, TSX, and JSX** in `crates/quench-runtime/`, executing `.ts/.tsx/.js/.jsx` natively with minimum code, maximum performance, and complete test coverage.

## Current State

### Test Status (2026-07-09)
- **Total tests**: 321
- **Passing**: 321
- **Failing**: 0

### File Size Issues (Priority P1)
- `builtins/promise/mod.rs`: 501 lines (exceeds 500)
- Total files: 18912 lines across ~50 files

### Conformance Status
| Suite | Subset | Pass Rate |
|-------|--------|-----------|
| TypeScript | 376 cases | 40.7% (153/376) |
| test262 | 431 files | 10.9% (47/431) |

## Implementation Plan

### Phase 1: Lint Enforcement (High Impact, Low Effort)
1. Add clippy lint rules to `.cargo/config.toml`
2. Split files exceeding 500 lines
3. Split functions exceeding 40 lines
4. Reduce complexity above 10

### Phase 2: Conformance Gaps (High Impact, High Effort)
1. Identify failing test262/TypeScript cases
2. Add targeted tests for each failure
3. Implement missing features

### Phase 3: Coverage Extension (High Impact, High Effort)
1. Expand test262 subset
2. Expand TypeScript conformance subset
3. Add scenario tests for real examples

## Verification

```bash
# Check passes
cargo check

# All tests pass
timeout 180 cargo test -p quench-runtime

# Examples work
cargo run -- examples/counter.js
cargo run -- examples/use-bridge.tsx
cargo run -- examples/animations.tsx
```

## Exit Criteria
- All tests pass (321/321)
- No files exceed 500 lines
- No functions exceed 40 lines
- Complexity ≤ 10 for all functions
- Conformance: TypeScript 100%, test262 100% on subset

## Notes
- Do not edit `tests/test262/`, `tests/typescript/`, `examples/`
- Commit after each successful step
- Track deferrals in this file
