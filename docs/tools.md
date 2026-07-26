# Development Tools for test262

## Quick reference

```bash
cargo run --bin run-test -- [--strict] [--module] [--stack] [--show-script] <test.js>
cargo run --bin inspect-test -- [--source] <test.js>   # metadata only

TEST262_STAGE=N cargo test -p quench-runtime --test test262 test262_staged -- --ignored
TEST262_STAGE=N TEST262_DIGEST=1 cargo test -p quench-runtime --test test262 test262_staged -- --ignored
ALL_STAGES=1 cargo test -p quench-runtime --test test262 test262_staged -- --ignored

TEST262_STAGE=N bash tools/run-each.sh        # process-isolated, parallel (release by default)
bash tools/digest-all.sh                      # digest all stages → report
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

> **Stale-binary hazard:** digest mode and `run-each.sh` execute a
> prebuilt `run-test` binary — the test harness does NOT rebuild it.
> The runner picks `target/release/run-test` when it exists, else
> `target/debug/run-test` (`RUN_TEST_BIN=<path>` overrides both).
> After changing `tools/run-test/` or runner judging logic, rebuild it
> first (`cargo build --release --bin run-test`) or the digest silently
> uses outdated judging.

## Release-mode digests (the fast path)

Digest mode spawns one `run-test` subprocess per test; per-test overhead
is ~40ms in debug vs ~10ms in release, so digests should run in release:

```bash
cargo build --release --bin run-test
TEST262_STAGE=N TEST262_DIGEST=1 cargo test --release -p quench-runtime --test test262 test262_staged -- --ignored
```

`tools/digest-all.sh` builds and uses the release binaries by default;
`TEST262_PROFILE=debug bash tools/digest-all.sh` opts back into debug.

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

One shared per-test timeout, `TEST_TIMEOUT_SECS = 15`
(`src/test262/runner/execute.rs`), used by both the in-process and the
isolated paths — a test cannot pass one way and fail the other. Isolated
runs (`run-each.sh`, default in digest) kill the subprocess after 15s →
Fail; `run-each.sh` enforces the same 15s via a perl watchdog (stock
macOS has no GNU `timeout`). Crash-looping files live in
`src/test262/skip.rs::CRASH_FILES` as **full stage-relative paths**;
each skip blocks stage completion until the crash is fixed.

## Workflow

1. Digest the current stage; pick the dominant cluster.
2. `cargo run --bin run-test -- <one test from it>` to see the exact
   failure (`--show-script` for the harnessed JS).
3. Reproducer `#[test]` → fix → re-digest (AGENTS.md cycle).
4. `bash tools/advance-stage.sh` at 100%.
