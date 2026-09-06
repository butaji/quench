# Performance lanes

The [stencil infrastructure gate](stencil-jit-implementation-spec.md) precedes
micros runs and benchmark-led tuning. Correctness tests continue during that work.

Build from the repository root into separate output directories:

```sh
cargo build --profile production -p quench-node
CARGO_TARGET_DIR=target-exec-trace cargo build --profile profiling -p quench-node --features execution-trace
```

- Control: `target/production/quench-node`, uninstrumented.
- Diagnostic: `target-exec-trace/profiling/quench-node`; enable
  `QUENCH_EXEC_TRACE=1` only in diagnostic processes.
- Record binary hashes, target, profile/features, source revision and dirty state.
  Paths and the current Git commit alone do not prove binary provenance.
- Compare baseline/candidate with matched build settings, inputs and idle-host
  conditions. Keep raw samples and per-case results; report uncertainty.
- Counts are not CPU time. Diagnostic or sampled runs never supply score evidence.
- Measure RSS separately from allocation counts, virtual memory and physical
  footprint; include generated code, static data and cache costs.
- A script wrapper is part of the tested artifact: record its source/hash and
  underlying executable. Do not silently substitute engines or output behavior.
- Review changes for [benchmark independence](benchmark-integrity.md); identifier
  searches alone cannot establish absence of benchmark-specific behavior.

Use the [micros manual](../quench-bench/micros/README.md) for commands and frozen
qualification rules, and [V8_v7](v8_v7.md) for full-suite measurement.
