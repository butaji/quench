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

# Run all stages in digest mode, produce report
bash tools/digest-all.sh

# Quick overview of all stages
bash tools/stage-status.sh

# Check if current stage is 100% and advance
TEST262_STAGE=16 bash tools/advance-stage.sh

# Machine-readable JSON output
TEST262_STAGE=16 TEST262_JSON=1 TEST262_DIGEST=1 cargo test -p quench-runtime --test test262
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

Fix the most numerous group first for maximum impact. The `$DONE` fix above unlocked 144 tests.

### 3. `TEST262_QUICK=1` — Quick Summary Mode

Shows just the pass/fail counts without per-test details.

```
TEST262_STAGE=16 TEST262_QUICK=1 TEST262_DIGEST=1 cargo test
```

### 4. `TEST262_JSON=1` — Machine-Readable Output

Produces JSON with failure groups for external processing (diffing, baselines, alerts).

```bash
TEST262_STAGE=16 TEST262_JSON=1 TEST262_DIGEST=1 cargo test > stage16.json
```

### 5. `tools/run-each.sh` — Process-Isolated Runner

Runs each test in a separate process using the `run-test` binary. Survives stack overflows and crashes that kill the in-process runner.

```bash
TEST262_STAGE=16 bash tools/run-each.sh
```

### 6. `tools/digest-all.sh` — Master Report

Runs digest on ALL 122 stages sequentially and produces a markdown report at `tasks/digest-report.md`. Stages that crash are noted separately.

```bash
bash tools/digest-all.sh
```

### 7. `tools/stage-status.sh` — Stage Overview

Shows all 122 stages with their status, test counts, and overall progress percentage.

```bash
bash tools/stage-status.sh
```

### 8. `tools/advance-stage.sh` — Auto-Advance

Checks if a stage has reached 100%, updates `tasks/index.json` to mark it done, and advances `current_stage` to the next pending stage.

```bash
TEST262_STAGE=16 bash tools/advance-stage.sh

# Or just check the current stage:
bash tools/advance-stage.sh
```

### 9. Per-Test Timeout

Every test has a 10-second timeout. Tests that hang are reported as "Must be optimized (timed out after 10s)".

## Workflow

### Fix by Root Cause (not by test)

1. Run digest on the current stage:
   ```bash
   TEST262_STAGE=16 TEST262_DIGEST=1 cargo test
   ```
2. Identify the largest failure group (e.g., "42 tests: Expected ReferenceError")
3. Fix the root cause (one fix may unlock 10-100+ tests)
4. Re-run digest to confirm improvement
5. When stage reaches 100%, auto-advance:
   ```bash
   bash tools/advance-stage.sh
   ```

### Debug a Single Test

```bash
# Quick: run in-process
cargo run --bin run-test -- path/to/failing-test.js

# If it crashes: use process isolation
TEST262_STAGE=N bash tools/run-each.sh
```

### Track Progress

```bash
bash tools/stage-status.sh
```

### Save Baseline and Compare

```bash
# Before fix
TEST262_STAGE=16 TEST262_JSON=1 TEST262_DIGEST=1 cargo test > before.json

# After fix
TEST262_STAGE=16 TEST262_JSON=1 TEST262_DIGEST=1 cargo test > after.json

# Diff (requires jq)
diff <(jq --sort-keys . before.json) <(jq --sort-keys . after.json)
```

## Config

- `TEST262_DIR` — path to test262 checkout (default: `tests/test262`)
- `TEST262_STAGE=N` — run only stage N
- `ALL_STAGES=1` — run all stages sequentially
- `TEST262_DIGEST=1` — collect all failures, don't stop at first
- `TEST262_QUICK=1` — minimal output, just counts
- `TEST262_JSON=1` — machine-readable JSON output
- `TEST262_FAILED_JSON=tasks/failures-N.json` — rerun only prior failures
- `TEST262_ISOLATED=1` — subprocess per test (build `run-test` first)
- `TEST262_PARALLEL=1` — parallel in-stage (default on)
- `TEST262_SERIAL=1` — force serial execution
- `TEST262_NOSKIP=1` — run crash-list files
- `RUN_TEST_BIN` — path to prebuilt `run-test` binary
- `RUST_BACKTRACE=1` — stack traces on panics

See also `tasks/harness-roadmap.md` for the full harness speed roadmap.

## Improving the Tools

These tools should evolve as you use them. Every time you run a digest, ask:

1. **Was the output useful?** If failure groups are too broad, improve the normalization.
2. **Did it crash?** Some tests cause stack overflows that kill the process. Add those to the skip list or improve the digest runner to use process isolation.
3. **Was it fast enough?** If it's too slow, add a mode that only samples the first N failures.
4. **Are the groups actionable?** If a group mixes different root causes, split by a deeper error analysis.
