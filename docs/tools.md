# Development Tools for test262

## Quick reference

```bash
cargo run --bin run-test -- [--strict] [--module] [--stack] [--show-script] <test.js>
cargo run --bin inspect-test -- [--source] <test.js>   # metadata only

TEST262_STAGE=N cargo test -p quench-runtime --test test262 test262_staged -- --ignored
TEST262_STAGE=N TEST262_DIGEST=1 cargo test -p quench-runtime --test test262 test262_staged -- --ignored
ALL_STAGES=1 cargo test -p quench-runtime --test test262 test262_staged -- --ignored

TEST262_STAGE=N bash tools/run-each.sh        # process-isolated (survives crashes)
bash tools/digest-all.sh                      # digest all 122 stages → report
bash tools/stage-status.sh                    # stage table + progress
bash tools/advance-stage.sh                   # mark current stage done if 100%
```

`TEST262_STAGE` defaults to `current_stage` in `tasks/index.json`.

## The gate (what "100%" means)

A stage is complete only when the staged run reports `ALL STAGES
COMPLETE`: zero failures **and zero skips**. Missing stage directories
fail the gate. `tools/advance-stage.sh` enforces the same rule before
editing `tasks/index.json`.

## `run-test` exit codes

| Code | Meaning |
|---|---|
| 0 | pass (negative tests: expected error type was thrown) |
| 1 | fail (assertion, wrong error type, async misuse) |
| 2 | usage error (bad flags) |
| 3 | negative test wrongly passed (no error thrown) |
| 4 | harness/build/read failure |

Signals (segfault/abort/stack overflow) are always failures, never
passes — including for negative tests.

## Digest mode — fix by root cause

```bash
TEST262_STAGE=N TEST262_DIGEST=1 cargo test -p quench-runtime --test test262 test262_staged -- --ignored
```

Collects ALL failures, groups them by error, writes
`tasks/failures-N.json`. Fix the largest group first.

| Env var | Effect |
|---|---|
| `TEST262_DUMP_FAILURES=<path>` | save failing test list |
| `TEST262_RERUN_FAILURES=<path>` | run only listed tests |
| `TEST262_FIRST_N=<N>` | first N tests only (smoke) |
| `TEST262_SHOW_SCRIPT=1` | dump generated harness+test JS on failure |
| `TEST262_JSON=1` | machine-readable digest on stdout |
| `TEST262_QUICK=1` | counts only |
| `TEST262_DIR=<path>` | test262 checkout (default `tests/test262`) |

## Timeouts and crashes

In-process: 10s per test. Isolated (`run-each.sh`, default in digest):
15s watchdog, then kill → Fail. Crash-looping files live in
`src/test262/skip.rs::CRASH_FILES` as **full stage-relative paths**;
each skip blocks stage completion until the crash is fixed.

## Workflow

1. Digest the current stage; pick the dominant cluster.
2. `cargo run --bin run-test -- <one test from it>` to see the exact
   failure (`--show-script` for the harnessed JS).
3. Reproducer `#[test]` → fix → re-digest (AGENTS.md cycle).
4. `bash tools/advance-stage.sh` at 100%.
