# Stage 44 Performance + Instrumentation Progress

- Stage: `test/language/expressions` (Stage 44)
- Updated: 2026-08-07
- Scope: Runner throughput and diagnostics only (no conformance rule changes)

## Plan status (17 items)

- [x] Add end-to-end digest timing payload with stage wall/execution timing plus test timing vectors.
- [x] Add slow-test reporting and output for top-N slowest tests.
- [x] Add per-stage metrics reset and collection plumbing in digest path.
- [x] Add fixture graph metrics counters (nodes, edges, max depth, selected modules).
- [x] Track worker start and worker batch counters for parallel scheduling behavior.
- [x] Emit full metric JSON payload including fixture cache counters and worker batch knobs.
- [x] Add optional metric log sink (`TEST262_METRICS_LOG`) with append semantics.
- [x] Add configurable worker batch sizing (`TEST262_WORKER_BATCH*`) controls and auto-scaling.
- [x] Add fixture load timing and module-load test counters already wired through metrics payload.
- [x] Tune fixture module traversal using deduplicating BFS state to avoid redundant loading and enable accurate graph telemetry.
- [x] Reduce digest memory/allocations by storing compact timing records (`index + enum outcome + elapsed`) and printing slow tests by resolving paths only at render time with lossless UTF-8 conversion.
- [x] Skip path-to-string conversion for all non-failing digest outcomes; allocate paths only when recording failures.
- [x] Preserve deterministic outcome ordering in parallel runs with an index-addressed `Vec<Option<TimedOutcome>>` and a single forward flatten pass (no final sort).
- [x] Remove redundant BFS `discovered` set in fixture traversal and reuse `selected_modules` as the dedupe frontier set.
- [x] Record process-isolation fallback count for panic-driven fallback paths.
- [x] Include `fixture_invalid_syntax_modules` and `isolation_fallbacks` in the digest metrics JSON payload.
- [x] Reduce failure-grouping allocations by cloning `TestFailure` only when detail mode is requested.

## Notes

- Progress is implemented in `crates/quench-test262/src/test262/runner/execute.rs` and `crates/quench-test262/src/test262/runner/digest.rs`.
- Stage-44 performance path is largely implemented end-to-end. Remaining work is to run and compare staged metrics after these changes (`TEST262_METRICS=1`) and keep a before/after snapshot.
- 2026-08-07: Ran Stage 44 (`test/language/expressions`) in digest mode with `TEST262_DIGEST=1 TEST262_METRICS=1`.
  - Stage: 44
  - Tests: 11,101
  - Wall: 390,678 ms
  - Execution: 390,683 ms
  - Passed: 11,038
  - Failed: 63
  - Skipped: 0
  - Slow-test threshold: 5 ms (`--show` showed top ~2.4s timeout case and many >100 ms TCO cases)
  - Main blockers:
    - 1 timeout on dynamic-import/catch nested ambiguous import test
    - 14+ assertion mismatches around destructuring yield/name/function-name cases and object spreads
    - 8 `TypeError` in spread/new/super paths
  - This confirms stage 44 remains non-green and needs runtime conformance fixes before full 50K+ run can progress.
