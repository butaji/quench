# 018 — Restore and run full test262 stage coverage

## Scope
- Identify a reliable full test262 execution path in this checkout.
- Run Stage 44 and larger stages with digest/metrics capture for throughput + memory checks.
- Iterate on runner path and runtime fixes until stage 44 and full suite can be measured continuously.

## Status
- `in_progress`

## Progress log
- 2026-08-07: Ran `cargo test -p quench-node` for available Rust tests.
  - Result: 2/2 tests passed.
- 2026-08-07: Probed test262 harness availability via `npx --yes test262-harness`.
  - Harness is available.
  - Probe showed `target/debug/quench-cli` exits with `ReferenceError: require is not defined` when exercised by the harness, because it is not a Node-host-compatible wrapper for test262 harness expectations.
- 2026-08-07: Next required step: restore or add a dedicated test262 host/runner entrypoint that handles harness contract (or wire the existing `quench-test262` runner path that produced these metrics targets), then rerun Stage 44 with `TEST262_METRICS=1` and capture baseline.
