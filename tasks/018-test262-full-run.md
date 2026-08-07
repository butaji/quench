# 018 — Restore and run full test262 stage coverage on quench-runtime

## Scope
- Recover the quench-runtime-native Test262 execution path from the preserved repository history.
- Run Stage 44 and larger stages with digest/metrics capture for throughput + memory checks.
- Iterate on runner path and runtime fixes until stage 44 and full suite can be measured continuously.

## Status
- `in_progress`

## Progress log
- 2026-08-07: User-directed pivot: stop pursuing the `quench-node` adapter path. The target is the preserved `quench-runtime` implementation and its native Test262 runner.
- 2026-08-07: Repository audit found the runnable runtime workspace and Test262 runner in the parent of commit `909c9363`; the current branch is the later Node-only state. The next implementation step is to recover that runtime-native path without modifying `tests/test262`.
- 2026-08-07: Ran `cargo test -p quench-node` for available Rust tests.
  - Result: 2/2 tests passed.
- 2026-08-07: Probed test262 harness availability via `npx --yes test262-harness`.
  - Harness is available.
  - Probe showed `target/debug/quench-cli` exits with `ReferenceError: require is not defined` when exercised by the harness, because it is not a Node-host-compatible wrapper for test262 harness expectations.
- 2026-08-07: Next required step: restore the buildable `quench-runtime` workspace/runner from repository history, then rerun Stage 44 with `TEST262_METRICS=1` and capture a fresh baseline.
