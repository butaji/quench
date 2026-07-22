# Development Tools for test262

## Quick Reference

```bash
# Run a single test with full diagnostics
cargo run --bin run-test -- tests/test262/.../test.js

# Run stage and show ALL failures grouped by root cause
TEST262_STAGE=16 TEST262_DIGEST=1 cargo test -p quench-runtime --test test262

# Run a single stage (stops at first failure)
TEST262_STAGE=16 cargo test -p quench-runtime --test test262

# Run all stages (stops at first failure)
ALL_STAGES=1 cargo test -p quench-runtime --test test262

# Process-isolated: survives stack overflows (slower)
TEST262_STAGE=16 bash tools/run-each.sh

# Run and show just pass/fail summary
TEST262_STAGE=16 TEST262_QUICK=1 cargo test -p quench-runtime --test test262
```

## Tool Reference

### 1. `run-test` — Single-Test Runner

```
cargo run --bin run-test -- <path-to-test.js>
```

Shows: test metadata (features, spec section, description), full source with line numbers, test result.

```bash
# Example
cargo run --bin run-test -- tests/test262/test/language/statements/class/name.js
```

### 2. `TEST262_DIGEST=1` — Failure Digest Mode

Runs ALL tests in a stage (or all stages), collects every failure, and groups them by error message. This is the most powerful tool for finding root causes.

```bash
# Digest a single stage
TEST262_STAGE=16 TEST262_DIGEST=1 cargo test -p quench-runtime --test test262

# Digest all stages  
ALL_STAGES=1 TEST262_DIGEST=1 cargo test -p quench-runtime --test test262
```

Output shows groups like:
```
────────────────────────────────────────────
  JsError("ReferenceError: $DONE is not defined")  (288 tests)
────────────────────────────────────────────
────────────────────────────────────────────
  JsError("Test262Error: Expected ReferenceError to be thrown...")  (42 tests)
────────────────────────────────────────────
```

Fix the most numerous group first for maximum impact.

### 3. `TEST262_QUICK=1` — Quick Summary Mode

Shows just the pass/fail counts without per-test details.

```
TEST262_STAGE=16 TEST262_QUICK=1 TEST262_DIGEST=1 cargo test -p quench-runtime --test test262
```

### 4. `tools/run-each.sh` — Process-Isolated Runner

Runs each test in a separate process. Survives stack overflows and crashes that kill the in-process runner.

```bash
TEST262_STAGE=16 bash tools/run-each.sh
```

### 5. `tools/run-test` — Standalone Binary (see #1)

### 6. Per-Test Timeout

Every test has a 10-second timeout. Tests that hang are reported as "Must be optimized (timed out after 10s)".

## Workflow

### Fix by Root Cause (not by test)

1. Run digest to find the most numerous failure group
2. Fix the root cause (one fix may unlock 10-100+ tests)
3. Re-run digest to confirm improvement
4. Repeat

Example digests from Stage 16 (class tests, 4367 tests):
- After $DONE fix: **744/844** passed, failures grouped
- Top groups: `Expected ReferenceError` (42), `Expected true but got false` (26), `Expected TypeError` (24)

### Debug a Single Test

```bash
# Quick: run in-process
cargo run --bin run-test -- path/to/failing-test.js

# If it crashes: use process isolation
TEST262_STAGE=N bash tools/run-each.sh
```

### Config

- `TEST262_DIR` — path to test262 checkout (default: `tests/test262`)
- `TEST262_STAGE=N` — run only stage N
- `ALL_STAGES=1` — run all stages sequentially
- `TEST262_DIGEST=1` — collect all failures, don't stop at first
- `TEST262_QUICK=1` — minimal output, just counts

RUST_BACKTRACE=1 can be combined with any mode for stack traces on panics.

## Improving the Tools

These tools should evolve as you use them. Every time you run a digest, ask:

1. **Was the output useful?** If failure groups are too broad, improve the normalization.
2. **Did it crash?** Some tests cause stack overflows that kill the process. Add those to the skip list or improve the digest runner to use process isolation.
3. **Was it fast enough?** If it's too slow, add a mode that only samples the first N failures.
4. **Are the groups actionable?** If a group mixes different root causes, split by a deeper error analysis.

### Planned improvements based on current limitations:

- **Process isolation per test**: The digest runner currently uses threads + catch_unwind, which cannot survive stack overflows (SIGABRT). A future version should spawn subprocesses for each test.
- **Failure sample mode**: `TEST262_SAMPLE=10` would stop after collecting 10 failure groups (faster for quick iteration).
- **Skip known failures**: `TEST262_SKIP_FILE=skip.txt` would read a list of known-failing tests and skip them during digest runs.
- **JSON output**: `TEST262_JSON=1` would output machine-readable failure data for external processing.
- **Regression detection**: Compare current digest against a baseline to show what's improved or regressed.
