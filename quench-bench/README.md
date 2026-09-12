# Benchmark harness

Fixtures and scoring are measurement-only; they never select VM behavior.

- [V8-v7](../docs/v8_v7.md): Rust runner, comparisons and limitations.
- [Performance protocol](../docs/performance-lanes.md): artifact identity and acceptance.
- [Micros manual](micros/README.md): semantic contrasts, timing, RSS and diagnostics.
- [Benchmark independence](../docs/benchmark-integrity.md): production invariants.

Run measurements at meaningful milestones after the affected execution path is
safe and correct. The [task queue](../tasks/index.json) records remaining work.

The V8 runner accepts `--all`, `--runs`, `--timeout-ms`, `--node`, `--bun`,
`--quench` and `--out`. `--out` uses create-new semantics and records source,
artifact, Git and toolchain identity plus per-fixture uncertainty; it refuses
tracing-enabled scoring environments.
