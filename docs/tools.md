# Development Tools for test262

## Quick Reference

```bash
# Run a single test with full diagnostics
cargo run --bin run-test -- [--strict] [--stack] [--show-script] tests/test262/.../test.js

# Inspect test metadata without running
cargo run --bin inspect-test -- [--source] tests/test262/.../test.js

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
cargo run --bin run-test -- [options] <path-to-test.js>
```

Runs a single test262 test with full diagnostics. Handles async, module, and strict mode automatically by reading the test's frontmatter.

**Options:**

| Flag | Description |
|------|-------------|
| `--strict` | Also run in strict mode (auto-detected from flags by default) |
| `--module` | Run as ES module (auto-detected from flags by default) |
| `--show-script` | Dump the full generated script (harness includes + test source) |
| `--stack` | Show full error debug output on failure |

**Examples:**

```bash
# Basic run with metadata + source display
cargo run --bin run-test -- tests/test262/test/language/statements/class/name.js

# Debug a cryptic failure — see the harnessed JS
cargo run --bin run-test -- --show-script path/to/failing-test.js

# Run with strict mode + stack trace
cargo run --bin run-test -- --strict --stack path/to/failing-test.js

# Force module mode
cargo run --bin run-test -- --module path/to/module-test.js
```

**Exit codes:**
- `0` — all variants passed
- `1` — one or more variants failed
- `2` — harness/build error, bad flags, file not found

### 2. `inspect-test` — Metadata Inspector

```
cargo run --bin inspect-test -- [--source] <path-to-test.js>
```

Shows parsed test262 frontmatter metadata WITHOUT running the test. Useful for quickly understanding what a test expects.

**Examples:**

```bash
# Show metadata summary
cargo run --bin inspect-test -- tests/test262/test/language/statements/class/super/in-constructor-superproperty-evaluation.js

# Show metadata + full source
cargo run --bin inspect-test -- --source path/to/test.js
```

Output includes: description, spec section, features, flags, included harness files, negative expectations (error type + phase), line/char count, and test structure hints (uses assert.*, throw, $DONE, export).

### 3. `TEST262_DIGEST=1` — Failure Digest Mode

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

**Diagnostic env-vars that work with digest:**

| Env Var | Effect |
|---------|--------|
| `TEST262_SHOW_SCRIPT=1` | Dump full generated JS script for each failing test |
| `TEST262_DUMP_FAILURES=<path>` | Save list of all failing tests to a file after digest |
| `TEST262_RERUN_FAILURES=<path>` | Only run tests listed in a previous failure file |
| `TEST262_FIRST_N=<N>` | Only run first N tests (ultra-fast smoke test) |

**Examples:**

```bash
# Show generated script for failing tests
TEST262_STAGE=16 TEST262_DIGEST=1 TEST262_SHOW_SCRIPT=1 cargo test

# Save failure list for later iteration
TEST262_STAGE=16 TEST262_DIGEST=1 TEST262_DUMP_FAILURES=/tmp/fails.txt cargo test

# Re-run only previously failed tests (fast!)
TEST262_STAGE=16 TEST262_DIGEST=1 TEST262_RERUN_FAILURES=/tmp/fails.txt cargo test

# Quick smoke test — first 10 tests only
TEST262_STAGE=16 TEST262_DIGEST=1 TEST262_FIRST_N=10 cargo test
```

### 4. `TEST262_QUICK=1` — Quick Summary Mode

Shows just the pass/fail counts without per-test details.

```
TEST262_STAGE=16 TEST262_QUICK=1 TEST262_DIGEST=1 cargo test
```

### 5. `TEST262_JSON=1` — Machine-Readable Output

Produces JSON with failure groups for external processing (diffing, baselines, alerts).

```bash
TEST262_STAGE=16 TEST262_JSON=1 TEST262_DIGEST=1 cargo test > stage16.json
```

### 6. `tools/run-each.sh` — Process-Isolated Runner

Runs each test in a separate process using the `run-test` binary. Survives stack overflows and crashes that kill the in-process runner.

```bash
TEST262_STAGE=16 bash tools/run-each.sh
```

### 7. `tools/digest-all.sh` — Master Report

Runs digest on ALL 122 stages sequentially and produces a markdown report at `tasks/digest-report.md`. Stages that crash are noted separately.

```bash
bash tools/digest-all.sh
```

### 8. `tools/stage-status.sh` — Stage Overview

Shows all 122 stages with their status, test counts, and overall progress percentage.

```bash
bash tools/stage-status.sh
```

### 9. `tools/advance-stage.sh` — Auto-Advance

Checks if a stage has reached 100%, updates `tasks/index.json` to mark it done, and advances `current_stage` to the next pending stage.

```bash
TEST262_STAGE=16 bash tools/advance-stage.sh

# Or just check the current stage:
bash tools/advance-stage.sh
```

### 10. Per-Test Timeout

Every test has a 10-second timeout. Tests that hang are reported as "Must be optimized (timed out after 10s)".

## Workflow

### Fix by Root Cause (not by test)

1. Run digest on the current stage:
   ```bash
   TEST262_STAGE=16 TEST262_DIGEST=1 cargo test
   ```
2. Identify the largest failure group (e.g., "42 tests: Expected ReferenceError")
3. Fix the root cause (one fix may unlock 10-100+ tests)
4. Save failure list and re-run only those:
   ```bash
   TEST262_STAGE=16 TEST262_DIGEST=1 TEST262_DUMP_FAILURES=/tmp/fails.txt cargo test
   # ... apply fix ...
   TEST262_STAGE=16 TEST262_DIGEST=1 TEST262_RERUN_FAILURES=/tmp/fails.txt cargo test
   ```
5. When stage reaches 100%, auto-advance:
   ```bash
   bash tools/advance-stage.sh
   ```

### Debug a Single Test

```bash
# Quick: inspect metadata
cargo run --bin inspect-test -- path/to/failing-test.js

# Run with source display
cargo run --bin run-test -- path/to/failing-test.js

# If it crashes: use process isolation
TEST262_STAGE=N bash tools/run-each.sh

# See the harness-generated script
cargo run --bin run-test -- --show-script path/to/failing-test.js

# See stack trace on failure
cargo run --bin run-test -- --stack path/to/failing-test.js
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

# Or use failure file for exact test lists:
TEST262_STAGE=16 TEST262_DIGEST=1 TEST262_DUMP_FAILURES=/tmp/before.txt cargo test
TEST262_STAGE=16 TEST262_DIGEST=1 TEST262_DUMP_FAILURES=/tmp/after.txt cargo test
diff /tmp/before.txt /tmp/after.txt
```

## Config

- `TEST262_DIR` — path to test262 checkout (default: `tests/test262`)
- `TEST262_STAGE=N` — run only stage N
- `ALL_STAGES=1` — run all stages sequentially
- `TEST262_DIGEST=1` — collect all failures, don't stop at first
- `TEST262_QUICK=1` — minimal output, just counts
- `TEST262_JSON=1` — machine-readable JSON output
- `TEST262_SHOW_SCRIPT=1` — dump generated JS script on failure
- `TEST262_DUMP_FAILURES=<path>` — save failure list to file
- `TEST262_RERUN_FAILURES=<path>` — re-run only listed tests
- `TEST262_FIRST_N=<N>` — only run first N tests
- `RUST_BACKTRACE=1` — stack traces on panics

See also `tasks/harness-roadmap.md` for the full harness speed roadmap.

## Improving the Tools

These tools should evolve as you use them. Every time you run a digest, ask:

1. **Was the output useful?** If failure groups are too broad, improve the normalization.
2. **Did it crash?** Some tests cause stack overflows that kill the process. Add those to the skip list or improve the digest runner to use process isolation.
3. **Was it fast enough?** If it's too slow, use `TEST262_FIRST_N=10` for quick iteration, or `TEST262_RERUN_FAILURES` to only run tests that previously failed.
4. **Are the groups actionable?** If a group mixes different root causes, split by a deeper error analysis.
5. **Can you inspect the failing JS?** Use `TEST262_SHOW_SCRIPT=1` to see the exact harnessed script the runtime evaluates.
6. **Can you understand the test quickly?** Use `cargo run --bin inspect-test -- path/to/test.js` to see parsed metadata without running.
