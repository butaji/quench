# Development Tools for test262

## Quick Reference

Terminology is single-source:
- `test run` is the canonical term for stage status and milestone progress.
- `progress` and `snapshot` refer to `test run` data.
- `ssot` is the canonical shorthand for `test run` (`ssot` == `test run`).
- `ssot` handles current-stage and next-pending-stage entrypoints (`--next` for queue navigation).
- `SSOT_BUILD_RUN_TEST` is supported for compatibility; use `TEST262_TEST_RUN_BUILD=1` for stage prebuild mode.

```bash
# Run a single test with full diagnostics (auto-shows error type, source context)
cargo run --bin run-test -- [--strict] [--stack] [--show-script] tests/test262/.../test.js

# Inspect test metadata without running
cargo run --bin inspect-test -- [--source] tests/test262/.../test.js

# Run stage and show ALL failures grouped by root cause (digest mode)
TEST262_STAGE=N TEST262_DIGEST=1 cargo test -p quench-runtime --test test262

# Digest + per-test detail (error type, source context, JS stack per failure)
TEST262_STAGE=N TEST262_DIGEST=1 TEST262_DETAIL=1 cargo test -p quench-runtime --test test262

# Run a single stage (stops at first failure)
TEST262_STAGE=N cargo test -p quench-runtime --test test262 test262_staged -- --nocapture

# Run all stages (stops at first failure)
ALL_STAGES=1 cargo test -p quench-runtime --test test262

# Process-isolated: survives stack overflows (slower)
TEST262_STAGE=N bash tools/run-each.sh

# Run and show just pass/fail summary
TEST262_STAGE=N TEST262_QUICK=1 cargo test -p quench-runtime --test test262

# Run all stages in digest mode, produce report
bash tools/digest-all.sh

# Quick overview of all stages
bash tools/stage-status.sh
# Current stage only (minimal output)
bash tools/stage-status.sh --current
# Canonical single-command test-run progress snapshot
bash tools/implementation-progress.sh
# Canonical one-line workflow for SSOT/test-run
bash tools/ssot
bash tools/ssot --status
bash tools/ssot --json
bash tools/ssot --next
bash tools/ssot --next --run
bash tools/ssot --next --run --commit --push
# Fast preflight guard before running stage test-run automation
bash tools/test-run-preflight.sh
# Preflight with machine-readable output
bash tools/test-run-preflight.sh --json
# Assert no pending work: exit 0 when all stages done, 1 otherwise
bash tools/implementation-progress.sh --ci
# Include next pending stage in the same snapshot
bash tools/implementation-progress.sh --next
# Explicitly request JSON-style output key (no behavior change, compatibility)
bash tools/implementation-progress.sh --json
# Include both next stage and CI gate in one output/exit code
bash tools/implementation-progress.sh --next --ci
# Add aggregate test-run summary fields (done/pending/progress)
bash tools/implementation-progress.sh --summary
# Compact status summary for current stage test-run
bash tools/test-run-status-summary.sh
# Fail with non-zero exit code only when current stage has failures
bash tools/test-run-status-summary.sh --blocker
# One-shot test-run dashboard (current + next + aggregate signals)
bash tools/test-run-dashboard.sh
# Dashboard JSON for automation
bash tools/test-run-dashboard.sh --json
# Assert ready-to-run gate (ready=true && blocked=false)
bash tools/test-run-dashboard.sh --assert-ready
# One-command cycle helper: summary + optional run of current stage
bash tools/test-run-cycle.sh
bash tools/test-run-cycle.sh --json
# Assert ready and run current stage immediately
bash tools/test-run-cycle.sh --assert-ready --run
# One-command preflight + run for current stage
bash tools/test-run-go.sh --run
# Run current stage with machine-readable preflight + run payloads
bash tools/test-run-go.sh --run --json
# One-command next-pending stage run (optionally by ratio)
bash tools/test-run-go-next.sh
bash tools/test-run-go-next.sh --run --json
bash tools/test-run-go-next.sh --run --by-ratio --top 5
bash tools/test-run-go-next.sh --run --advance
bash tools/test-run-go-next.sh --print
bash tools/test-run-go-next.sh --print-json
# Status payload now includes current/match/advance-eligible fields
bash tools/test-run-go-next.sh --status
# Fastest next-stage path: JSON + auto-advance
bash tools/test-run-go-next-fast.sh
bash tools/test-run-go-next-fast.sh --no-preflight
bash tools/test-run-go-next-fast.sh --by-ratio --top 5
# Dry-run selected next-stage flow (metadata + optional readiness gate)
bash tools/test-run-go-next-dryrun.sh
bash tools/test-run-go-next-dryrun.sh --print-json
bash tools/test-run-go-next-dryrun.sh --assert-ready
bash tools/test-run-go-next-dryrun.sh --run-check
# CI gate for next-stage readiness.
# Default: human-readable checks (asserts current-stage and auto-advance readiness).
bash tools/test-run-go-next-ci.sh
# JSON output for CI/parsers (same checks, machine-readable envelope).
bash tools/test-run-go-next-ci.sh --json
# Strict machine-readable envelope only.
bash tools/test-run-go-next-ci.sh --json-only
# Unified CI gate for both current + next stage readiness.
bash tools/test-run-ci-gate.sh
bash tools/test-run-ci-gate.sh --json
bash tools/test-run-ci-gate.sh --skip-next --json
bash tools/test-run-ci-gate.sh --run
# Run current stage immediately if readiness checks pass
bash tools/test-run-ci-gate.sh --run --skip-next
# Single JSON readout with optional immediate run
bash tools/test-run-go.sh --ready --json
# Fast mode: skip preflight when reruns are intentionally unsafe
bash tools/test-run-go.sh --run --no-preflight
# Raw script output (human-readable)
bash tools/implementation-progress.sh --raw
# Milestone workflow shortcut for current-stage status
bash tools/milestone.sh --status --current
# One-shot milestone status + CI gate readout (use --json for machine-readable)
bash tools/milestone.sh --status --ci
# Machine-readable combined readout
bash tools/milestone.sh --status --ci --json
# Scope-specific combined readout
bash tools/milestone.sh --status --ci --current --json
bash tools/milestone.sh --status --ci --next --json
bash tools/milestone.sh --status --ci --next-id
# Raw combined readout
bash tools/milestone.sh --status --ci --raw
# `--json` takes precedence when both --json and --raw are provided.
# Returned as JSON fields: `status_scope`, `status`, `status_rc`, `ci`, `ci_rc`, and `ok`.
# CI readiness gate from milestone automation
bash tools/milestone.sh --ci-gate
bash tools/milestone.sh --ci-gate --ci-gate-json
bash tools/milestone.sh --ci-gate --json
bash tools/milestone.sh --ci-gate --run --skip-next
bash tools/milestone.sh --ci-gate --run --by-ratio --top 5
# Next pending stage (actionable target)
bash tools/stage-status.sh --next
bash tools/milestone.sh --status --next
# Script-friendly next-stage id only
bash tools/stage-status.sh --next-id
bash tools/milestone.sh --status --next-id
# direct helper (prints stage id, non-zero exit if none)
bash tools/next-stage.sh
# Run the next pending stage (auto-resolves stage id, supports --json/--build)
bash tools/run-next-pending.sh
# Run a unified plan selector for next stage or batch selection
bash tools/run-test-plan.sh
# Canonical test-run status entrypoint (always machine-readable by default)
bash tools/run-test-plan-status.sh
# Raw legacy payload for older automation
bash tools/run-test-plan-status.sh --raw
# Canonical stage test-run status entrypoint
bash tools/test-run-status.sh
# Stage test-run status in raw human mode
bash tools/test-run-status.sh --raw
# Run and persist a machine-readable plan execution record
bash tools/run-test-plan-log.sh --status-json
# Log only compact status summary (no command output replay)
bash tools/run-test-plan-log.sh --status --json --summary-only
# Log only compact status summary with normalized wrapper
bash tools/run-test-plan-log.sh --status-json --summary-only
# Include raw output snippet in logged summary
bash tools/run-test-plan-log.sh --status --json --raw
# Print resolved stage id without running
bash tools/run-next-pending.sh --print
# Print resolved stage as JSON metadata (source/id/path)
bash tools/run-next-pending.sh --print-json
# Test-run metadata for next pending stage (same payload as --print-json)
bash tools/run-next-pending.sh --status
# Normalized status-only plan metadata (single/batch)
bash tools/run-test-plan.sh --status-json
bash tools/run-test-plan.sh --batch --top 5 --status --json
bash tools/run-test-plan.sh --batch --top 5 --status-json
# Emit native payload (non-normalized), useful for older automation
bash tools/run-test-plan.sh --batch --top 5 --status --json --raw
# Fail-fast schema validation on status payload
bash tools/run-test-plan.sh --batch --top 5 --status --json --strict
# Run highest-failure-ratio pending stage
bash tools/run-next-pending.sh --by-ratio
# Restrict ratio-driven pick to top-N candidate window
bash tools/run-next-pending.sh --by-ratio --top 20
# Run one batch selection from the same entrypoint (status payload)
bash tools/run-test-plan.sh --batch --ratio --top 10 --status
# Run one batch selection with machine-readable status payload + raw JSON
bash tools/run-test-plan.sh --batch --ratio --top 10 --run --json
# List all pending stages (id-only output)
bash tools/pending-stages.sh
# List top 10 highest-failure pending stages (human table)
bash tools/pending-stages.sh --top 10 --verbose
# Top 10 pending stages as JSON
bash tools/pending-stages.sh --top 10 --json
# Top 10 by failure ratio (failed/tests)
bash tools/pending-stages.sh --top-ratio 10
bash tools/pending-stages.sh --top-ratio 10 --json
# Plan or execute a pending stage batch (default top-3 by failed)
bash tools/run-pending-batch.sh
bash tools/run-pending-batch.sh --ratio --top 5
bash tools/run-pending-batch.sh --ratio --top 5 --run
# Machine-readable status object for batch plan
bash tools/run-pending-batch.sh --ratio --top 5 --status
# Include wrapped raw JSON payload alongside the status object
bash tools/run-pending-batch.sh --ratio --top 5 --run --status --json
# Stop immediately when first stage in batch fails
bash tools/run-pending-batch.sh --ratio --run --stop-on-fail
# Allow at most N failures before aborting (0 = unlimited, default)
bash tools/run-pending-batch.sh --ratio --run --max-failures 2
# Quick aggregate remaining-work summary
bash tools/pending-stages.sh --summary
bash tools/pending-stages.sh --summary --json
bash tools/pending-stages.sh --json

# Diagnose current stage quickly (digest + first failure run-test)
TEST262_STAGE=N bash tools/fix-stage.sh

# Check if current stage is 100% and advance
TEST262_STAGE=N bash tools/advance-stage.sh

# Canonical status command for active work (stage test run)
bash tools/test-run-stage.sh [stage]
# Machine-readable mode for automation
bash tools/test-run-stage.sh --json [stage]
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
| `--stack` | Show full Rust-level error debug output on failure |
| `--inspect EXPR` | Evaluate a JS expression in the failed context and print its value |

**On failure, `run-test` auto-shows:**

- Error type (TypeError, Test262Error, ReferenceError, etc.)
- Error message (the `.message` property of the JS error object)
- Reason (the full error diagnostic)
- JS stack trace (extracted from the error object's `.stack` property if available)
- Source context (lines surrounding the point of failure in the test file)
- Inspection results (for `--inspect EXPR`)

**Examples:**

```bash
# Basic run — shows metadata, source, and rich diagnostics on failure
cargo run --bin run-test -- tests/test262/test/language/statements/class/name.js

# Debug a cryptic failure — see the harnessed JS
cargo run --bin run-test -- --show-script path/to/failing-test.js

# Run with Rust-level debug output on failure
cargo run --bin run-test -- --stack path/to/failing-test.js

# Inspect a variable after failure
cargo run --bin run-test -- --inspect 'typeof x' path/to/failing-test.js

# Force module mode
cargo run --bin run-test -- --module path/to/module-test.js
```

**Exit codes:**
- `0` — all variants passed
- `1` — one or more variants failed
- `2` — harness/build error, bad flags, file not found
- `3` — negative test wrongly passed (expected error, none occurred)
- `4` — infrastructure failure (read, build, context creation)

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

Output includes: description, spec section, features, flags, included harness files, negative expectations (error type + phase), line/char count, and test structure hints (uses assert.\*, throw, $DONE, export).

### 3. `TEST262_DIGEST=1` — Failure Digest Mode

Runs ALL tests in a stage (or all stages), collects every failure, and groups them by normalized error message. This is the most powerful tool for finding root causes.

```bash
# Digest a single stage
TEST262_STAGE=N TEST262_DIGEST=1 cargo test -p quench-runtime --test test262

# Digest all stages  
ALL_STAGES=1 TEST262_DIGEST=1 cargo test -p quench-runtime --test test262
```

Output shows groups like:

```json
{
  "stage": 0,
  "path": "test/harness",
  "passed": 102,
  "failed": 14,
  "skipped": 0,
  "total": 116,
  "groups": [
    {
      "reason": "Expected a ReferenceError to be thrown but got a TypeError",
      "count": 42,
      "sample_paths": ["tests/.../test.js"]
    }
  ]
}
```

Rich per-test diagnostics can be included with `TEST262_DETAIL=1`:

```bash
TEST262_STAGE=N TEST262_DIGEST=1 TEST262_DETAIL=1 cargo test
```

In detail mode, the JSON includes per-test diagnostic snapshots with `error_type`, `error_message`, `js_stack`, and `source_context`. A human-readable summary is also printed after the JSON.

**Diagnostic env-vars that work with digest:**

| Env Var | Effect |
|---------|--------|
| `TEST262_DETAIL=1` | Include per-test diagnostic snapshots in digest output |
| `TEST262_SHOW_SCRIPT=1` | Dump full generated JS script for each failing test |
| `TEST262_DUMP_FAILURES=<path>` | Save list of all failing tests to a file after digest |
| `TEST262_RERUN_FAILURES=<path>` | Only run tests listed in a previous failure file |
| `TEST262_FIRST_N=<N>` | Only run first N tests (ultra-fast smoke test) |
| `TEST262_FAILED_JSON=<path>` | Re-run only paths from a prior `tasks/failures-N.json` digest |

**Examples:**

```bash
# Digest with per-test detail (shows source context, error type, stack per failure)
TEST262_STAGE=N TEST262_DIGEST=1 TEST262_DETAIL=1 cargo test

# Show generated script for failing tests
TEST262_STAGE=N TEST262_DIGEST=1 TEST262_SHOW_SCRIPT=1 cargo test

# Save failure list for later iteration
TEST262_STAGE=N TEST262_DIGEST=1 TEST262_DUMP_FAILURES=/tmp/fails.txt cargo test

# Re-run only previously failed tests (fast!)
TEST262_STAGE=N TEST262_DIGEST=1 TEST262_RERUN_FAILURES=/tmp/fails.txt cargo test

# Quick smoke test — first 10 tests only
TEST262_STAGE=N TEST262_DIGEST=1 TEST262_FIRST_N=10 cargo test
```

### 4. `TEST262_QUICK=1` — Quick Summary Mode

Shows just the pass/fail counts without per-test details.

```
TEST262_STAGE=N TEST262_QUICK=1 TEST262_DIGEST=1 cargo test
```

### 5. `TEST262_JSON=1` — Machine-Readable Output

Produces JSON with failure groups for external processing (diffing, baselines, alerts).

```bash
TEST262_STAGE=N TEST262_JSON=1 TEST262_DIGEST=1 cargo test > stage16.json
```

The JSON output now includes structured diagnostics per test when `TEST262_DETAIL=1`:

```json
{
  "groups": [{
    "reason": "sameValue failed N !== N",
    "count": 10,
    "samples": [{
      "path": "tests/test262/test/.../test.js",
      "error_type": "Test262Error",
      "error_message": "sameValue failed: actual 5 !== expected 3",
      "js_stack": null,
      "source_context": " →  42: assert.sameValue(f(), 3);"
    }]
  }]
}
```

### 6. `tools/run-each.sh` — Process-Isolated Runner

Runs each test in a separate process using the `run-test` binary. Survives stack overflows and crashes that kill the in-process runner.

```bash
TEST262_STAGE=N bash tools/run-each.sh
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
TEST262_STAGE=N bash tools/advance-stage.sh

# Or just check the current stage:
bash tools/advance-stage.sh
```

### 10. Per-Test Timeout

Every test has a 15-second timeout. Tests that hang are reported as "Must be optimized (timed out after 15s)".

## Workflow

### Fix by Root Cause (not by test)

1. Run digest on the current stage:
   ```bash
   TEST262_STAGE=N TEST262_DIGEST=1 cargo test
   ```
2. Identify the largest failure group (e.g., "42 tests: Expected ReferenceError")
3. Fix the root cause (one fix may unlock 10-100+ tests)
4. Re-run digest to verify progress:
   ```bash
   TEST262_STAGE=N TEST262_DIGEST=1 cargo test
   ```
5. When the largest group is fixed, re-run with per-test detail:
   ```bash
   TEST262_STAGE=N TEST262_DIGEST=1 TEST262_DETAIL=1 cargo test
   ```
6. When stage reaches 100%, auto-advance:
   ```bash
   bash tools/advance-stage.sh
   ```

### Debug a Single Test

```bash
# Quick: inspect metadata
cargo run --bin inspect-test -- path/to/failing-test.js

# Run with rich diagnostics (auto-shows error type, source context)
cargo run --bin run-test -- path/to/failing-test.js

# If it crashes: use process isolation
TEST262_STAGE=N bash tools/run-each.sh

# See the harness-generated script
cargo run --bin run-test -- --show-script path/to/failing-test.js

# See Rust-level debug output
cargo run --bin run-test -- --stack path/to/failing-test.js

# Inspect variables after failure
cargo run --bin run-test -- --inspect 'x' --inspect 'typeof y' path/to/failing-test.js
```

### Track Progress

```bash
bash tools/stage-status.sh
```

### Save Baseline and Compare

```bash
# Before fix
TEST262_STAGE=N TEST262_DIGEST=1 TEST262_DETAIL=1 cargo test > before.json

# After fix
TEST262_STAGE=N TEST262_DIGEST=1 TEST262_DETAIL=1 cargo test > after.json

# Diff (requires jq)
diff <(jq --sort-keys . before.json) <(jq --sort-keys . after.json)

# Or use failure file for exact test lists:
TEST262_STAGE=N TEST262_DIGEST=1 TEST262_DUMP_FAILURES=/tmp/before.txt cargo test
TEST262_STAGE=N TEST262_DIGEST=1 TEST262_DUMP_FAILURES=/tmp/after.txt cargo test
diff /tmp/before.txt /tmp/after.txt
```

## Config

- `TEST262_DIR` — path to test262 checkout (default: `tests/test262`)
- `TEST262_STAGE=N` — run only stage N
- `ALL_STAGES=1` — run all stages sequentially
- `TEST262_DIGEST=1` — collect all failures, don't stop at first
- `TEST262_DETAIL=1` — per-test diagnostic snapshots (error type, source context, stack)
- `TEST262_QUICK=1` — minimal output, just counts
- `TEST262_JSON=1` — machine-readable JSON output
- `TEST262_SHOW_SCRIPT=1` — dump generated JS script on failure
- `TEST262_DUMP_FAILURES=<path>` — save failure list to file
- `TEST262_RERUN_FAILURES=<path>` — re-run only listed tests
- `TEST262_FIRST_N=<N>` — only run first N tests
- `TEST262_FAILED_JSON=<path>` — re-run only paths from a prior digest JSON
- `TEST262_SERIAL=1` — disable parallel execution
- `TEST262_INPROCESS=1` — run digest in-process (risk of crash on stack overflow)
- `RUST_BACKTRACE=1` — Rust stack traces on panics

## Architecture

### Structured Failure Diagnostics

Every test failure is captured as a `TestFailure` struct with:

| Field | Type | Description |
|-------|------|-------------|
| `message` | `String` | Main error message (used for digest grouping) |
| `error_type` | `Option<String>` | JS error type, e.g. `TypeError`, `Test262Error` |
| `error_message` | `Option<String>` | JS error object's `.message` property |
| `js_stack` | `Option<String>` | JS stack trace from error object's `.stack` |
| `source_path` | `Option<String>` | Path to the test source file |
| `source_line` | `Option<usize>` | Approximate line of failure (1-based) |
| `source_context` | `String` | Surrounding source lines with line numbers |

The `error_type`, `error_message`, and `js_stack` fields are extracted from the thrown JS Value (error object) immediately after the failed eval, before any subsequent eval can overwrite the thread-local. This means even when the runtime doesn't set `.stack` on error objects, the fields are `None` — but when it does, they are captured and displayed automatically.

### Failure Flow

1. JS test throws → `ControlFlow::Throw(Value)` → `set_thrown_value(Value)`
2. `ctx.eval()` returns `Err(JsError("TypeError: ..."))`
3. `capture_thrown_diagnostics()` reads the thrown JS Value from thread-local,
   extracts `.name`, `.message`, `.stack` properties
4. `TestFailure` is constructed with the diagnostics + source context
5. `TestOutcome::Fail { failure }` carries it to the runner/display layer
6. `run-test` or digest mode renders it with type, message, source, stack

## Improving the Tools

These tools should evolve as you use them. Every time you run a digest, ask:

1. **Was the output useful?** If failure groups are too broad, improve the normalization (see `digest.rs::normalize_reason`).
2. **Did it crash?** Some tests cause stack overflows that kill the process. Add those to the skip list or use `TEST262_SERIAL=1` / `TEST262_INPROCESS=1` to debug.
3. **Was it fast enough?** If it's too slow, use `TEST262_FIRST_N=10` for quick iteration, or `TEST262_RERUN_FAILURES` to only run tests that previously failed.
4. **Are the groups actionable?** If a group mixes different root causes, split by a deeper error analysis. Use `TEST262_DETAIL=1` to see per-test source context and error types.
5. **Can you inspect the failing JS?** Use `TEST262_SHOW_SCRIPT=1` to see the exact harnessed script the runtime evaluates, or `cargo run --bin run-test -- path/to/test.js` for rich diagnostics.
6. **Can you understand the test quickly?** Use `cargo run --bin inspect-test -- path/to/test.js` to see parsed metadata without running.
