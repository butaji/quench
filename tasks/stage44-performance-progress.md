# Stage 44 Performance + Instrumentation Progress

Runner throughput and diagnostics for pure `quench-runtime` through the
`quench-test262` boundary. No conformance rule changes.

## Status (2026-08-08)

Done: digest timing payload (stage wall/execution + per-test vectors),
top-N slow-test reporting, fixture graph metrics (nodes/edges/depth),
worker batch counters and auto-scaling knobs (`TEST262_WORKER_BATCH*`),
metrics log sink (`TEST262_METRICS_LOG`), memory reductions in the digest
path (compact timing records, paths allocated only for failures,
deterministic index-ordered parallel outcomes), isolation-fallback
counters in the metrics JSON.

## Open

- [ ] Run and compare staged metrics (`TEST262_METRICS=1`) on the native
  runner to validate the stage-44 throughput work end to end.

## Notes

- Implementation currently lives in the transitional
  `crates/quench-runtime/src/test262/runner/` module; extraction to
  `quench-test262` is part of `tasks/019-runtime-boundaries.md`.
- The pure `quench-runtime` engine is the only target host (pivot away
  from Node adapters, 2026-08-07).
