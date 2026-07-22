# 10 Ways to Speed Up test262 Implementation

## Current Status
- **Stage**: 16 (test/language/statements/class - in_progress)
- **Total stages**: 121
- **Remaining**: ~105 stages
- **Target**: 100% test262 conformance

## Speed-up Strategies

### 1. Parallel Stage Testing (HIGH IMPACT)
**Problem**: Stages run sequentially, each waiting for the previous to complete.
**Solution**: When stages have no interdependencies, run them in parallel using `cargo test` with `--test-threads=N`.
**Expected Gain**: ~4-8x faster on multi-core machines.
**Implementation**: Modify `test262/runner.rs` to detect independent stages and run them concurrently.

### 2. Batch Fix by Category (HIGH IMPACT)
**Problem**: Random test failures across unrelated features.
**Solution**: Group failures by category (e.g., all Promise failures, all Array failures) and fix them in batches.
**Expected Gain**: Reduces context-switching overhead between different code areas.
**Implementation**: Add `tasks/failures-categorized.md` to track failures by category.

### 3. OXC Async-to-Generator Transform (MEDIUM IMPACT)
**Problem**: async/await requires complex suspend/resume machinery.
**Solution**: Use OXC's `async-to-generator` transform to convert async/await to generators, then implement generators once.
**Expected Gain**: ~2000 fewer lines of runtime code, handles all async patterns.
**Implementation**: Add OXC transformer pass before evaluation.

### 4. Self-Hosted Builtins in JS (MEDIUM IMPACT)
**Problem**: Rust builtins are verbose and hard to maintain.
**Solution**: Move pure-spec algorithms to JS (`builtins/*.js`), keeping only primitives in Rust.
**Expected Gain**: ~3x fewer LOC for builtins, easier to fix spec bugs.
**Implementation**: See `tasks/refactor-plan.md` R0.

### 5. Comprehensive Unit Test Coverage (MEDIUM IMPACT)
**Problem**: Debugging without tests is slow guesswork.
**Solution**: TDD for all Rust core functions before fixing test262 failures.
**Expected Gain**: Faster debugging, regression prevention.
**Implementation**: Add `#[test]` for every function in `src/eval/` and `src/value/`.

### 6. Better Test Harness Parallelism (LOW-MEDIUM IMPACT)
**Problem**: Current harness may not fully utilize available cores.
**Solution**: Profile and optimize the test runner's thread pool configuration.
**Expected Gain**: 10-20% faster harness execution.
**Implementation**: Add `#[tokio::test]` or custom thread pool configuration.

### 7. Incremental Compilation Caching (LOW IMPACT)
**Problem**: Rebuilding unchanged code on every test run.
**Solution**: Ensure `cargo build` uses incremental compilation properly.
**Expected Gain**: 30-60% faster rebuilds.
**Implementation**: Already enabled by default in cargo, but verify no cache invalidation issues.

### 8. Targeted Profiling to Find Hot Paths (LOW IMPACT)
**Problem**: Optimization efforts may target wrong code paths.
**Solution**: Use `cargo flamegraph` to identify actual bottlenecks.
**Expected Gain**: Focus optimization efforts on real hot paths.
**Implementation**: Run profiling on representative test262 subset.

### 9. Delete Dead Code (LOW IMPACT)
**Problem**: Accumulated unused code slows compilation and confuses maintenance.
**Solution**: Remove items from R4-R9 of `tasks/refactor-plan.md`.
**Expected Gain**: Cleaner codebase, faster compilation.
**Implementation**: Systematic dead code removal in each PR.

### 10. Automate Regression Detection (LOW IMPACT)
**Problem**: Fixes may break previously passing tests.
**Solution**: Run full test suite on every PR, not just affected stages.
**Expected Gain**: Catch regressions before they land.
**Implementation**: Add CI step to run all stages before merge.

---

## Prioritized Implementation Order

1. **Unit Test Coverage** (Strategy 5) - Do this FIRST, it's the foundation
2. **Batch Fix by Category** (Strategy 2) - Quick wins, visible progress
3. **Parallel Stage Testing** (Strategy 1) - Significant speedup for the test loop
4. **Self-Hosted Builtins** (Strategy 4) - Long-term maintainability
5. **OXC Async Transform** (Strategy 3) - Handles async/await without custom impl

---

## Tracking Progress

| Strategy | Status | Priority |
|----------|--------|----------|
| Unit Test Coverage | Pending | P0 |
| Batch Fix by Category | Pending | P1 |
| Parallel Stage Testing | Pending | P1 |
| Self-Hosted Builtins | Pending | P2 |
| OXC Async Transform | Pending | P2 |
| Harness Parallelism | Pending | P3 |
| Incremental Caching | Pending | P3 |
| Targeted Profiling | Pending | P3 |
| Delete Dead Code | Pending | P4 |
| Regression Detection | Pending | P4 |
