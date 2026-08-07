# 018 — Restore and run full test262 stage coverage

## Scope
- Identify a reliable full test262 execution path in this checkout.
- Run Stage 44 and larger stages with digest/metrics capture for throughput + memory checks.
- Iterate on runner path and runtime fixes until stage 44 and full suite can be measured continuously.

## Status
- `in_progress`

## Progress log
- 2026-08-07: Audited the current workspace and repository history.
  - `Cargo.toml` contains only `quench-node`; there is no buildable `quench-test262` package or configured Test262 command.
  - The tracked `crates/quench-test262` files are orphaned runner source fragments, not a runnable crate.
  - Commit `909c9363` intentionally removed the former `quench-runtime`/Test262 workspace, and `AGENTS.md` prohibits restoring that architecture.
  - This rules out wiring the old runner back in as a minimal fix. The remaining implementation boundary is a dedicated Test262 adapter built on `quench-node`.
- 2026-08-07: Ran `cargo test -p quench-node` for available Rust tests.
  - Result: 2/2 tests passed.
- 2026-08-07: Probed test262 harness availability via `npx --yes test262-harness`.
  - Harness is available.
  - Probe showed `target/debug/quench-cli` exits with `ReferenceError: require is not defined` when exercised by the harness, because it is not a Node-host-compatible wrapper for test262 harness expectations.
- 2026-08-07: Next required step: add a dedicated Test262 adapter/entrypoint on top of `quench-node` that handles the harness contract, then rerun Stage 44 with `TEST262_METRICS=1` and capture a fresh baseline. Do not restore the removed `quench-runtime` workspace.
