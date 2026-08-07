# 018 — Establish the quench-runtime / quench-test262 boundary

## Scope
- Keep `quench-runtime` pure: OXC AST → Quench IR → interpreter.
- Keep `quench-test262` responsible for metadata, harness, staging,
  isolation, metrics, and reports.
- Connect the two crates only through the public host execution interface.
- Run Stage 44 and larger stages with digest/metrics capture for throughput + memory checks.
- Iterate on runner path and runtime fixes until stage 44 and full suite can be measured continuously.

## Status
- `in_progress`

## Current checkpoint
- Engine pipeline: present as parser/lowering/interpreter in
  `quench-runtime`; compact IR boundary is the next explicit extraction
  target.
- Runner pipeline: present under `quench-runtime/src/test262`; standalone
  `quench-test262` crate is not yet present.
- Conformance: Stage 44 has a recorded native baseline but is not green.

## Progress log
- 2026-08-07: Node adapters are out of scope. The target is the pure
  `quench-runtime` engine and a separately bounded `quench-test262` runner.
- 2026-08-07: The current checkout has a transitional runner under
  `quench-runtime/src/test262`; extraction to `quench-test262` is required.
- 2026-08-07: Probed test262 harness availability via `npx --yes test262-harness`.
  - Harness is available.
  - Probe showed `target/debug/quench-cli` exits with `ReferenceError: require is not defined` when exercised by the harness, because it is not a Node-host-compatible wrapper for test262 harness expectations.
- 2026-08-07: Next step: extract the runner boundary without moving engine
  behavior, then rerun Stage 44 with `TEST262_METRICS=1`.
