# Performance protocol

V8-v7 is the primary progress indicator on Apple M4/macOS, with Bun/JSC and Node
comparisons. Measure at meaningful milestones after the affected path passes its
safety/correctness checks. Evidence starts with the first affected workload and
continues at every critical-path milestone; it does not replace the implementation
order in `tasks/index.json`, and there is no global infrastructure-first gate that
prevents earlier diagnostic or focused production measurements.

## Artifacts

```sh
cargo build --profile production -p quench-node
CARGO_TARGET_DIR=target-exec-trace cargo build --profile profiling -p quench-node --features execution-trace
```

Capture source revision and dirty diff, target/CPU/features, rustc version, build
profile/environment and SHA-256 at build time. Preserve independently named baseline
and candidate executables before rebuilding. Record comparator versions/hashes,
fixture inventory, commands and host conditions. A path or Git revision alone
does not identify a binary.

Scored runs use uninstrumented artifacts with tracing variables removed.
Diagnostics use separate artifacts and may enable `QUENCH_EXEC_TRACE=1`.
CPU samples, counters and compiler disassembly support attribution; instrumented
elapsed time cannot supply production score evidence.

Architecture-specific stencil opt-ins (including
`QUENCH_AARCH64_STENCIL_MODE`) are diagnostic capabilities until they pass the
same full 342-contract run, native-entry witnesses and M4 measurement checks as
the production policy. The default Apple-arm policy is likewise diagnostic
while broad native residency remains unqualified. A mode that changes canonical
semantic IR or lacks a complete fallback proof is not a qualified candidate and
must remain opt-in. The current mode-by-mode status and known failures have one authority in the
[execution-profile contract](execution-contract-tests.md); keep those modes
diagnostic and do not use their output for JSON regeneration or score claims.

## Comparisons

Run sequentially on an otherwise idle host; finish builds first. Alternate
baseline/candidate order across independent repetitions. Start with at least five
runs per artifact; add repetitions when uncertainty prevents a conclusion. Preserve
every attempt at a new output path, including failures.

For each engine, report per-fixture median scores and the geometric mean across
all eight valid fixture medians. Report variability and 95% confidence intervals
for matched candidate/baseline ratios. Missing/invalid scores prohibit a full-suite
claim. A five-second timeout is not evidence of a semantic defect; inspect output
and rerun with a documented adequate bound. The Rust runner records the invalid
round and advances to the next fixture instead of spending all remaining repeats
on a fixture that is already incomplete.

A performance task closes on a reproducible improvement beyond uncertainty,
unchanged semantics, and investigation of every per-fixture regression.
Fix unexplained regressions before acceptance; any retained tradeoff must be
quantified and justified against the task's declared objective. Functional
infrastructure tasks require executable proof but need not claim speed gains.

Measure startup, compile time, generated-code/cache bytes and RSS separately.
Adaptive suite process peaks do not establish equal-work memory improvements;
use fixed-work retained/released controls for allocation and reclamation changes.
Unavailable hardware counters are unknown, never zero.

## Existing runners and gaps

[V8-v7 commands](v8_v7.md) describe available raw runners.
[Micros](../quench-bench/micros/README.md) provide semantic contrasts, timing,
fixed-work memory and diagnostic adapters. Their frozen qualification thresholds
are that edition's protocol, not architectural limits or VM readiness criteria.

The Rust V8 runner now provides balanced engine order, complete build/source
provenance and score uncertainty in its schema-2 report. The V8-v7 runner
command in [V8-v7 commands](v8_v7.md) is an evidence milestone only when it
produces eight fixtures with repeated valid samples and the runtime/Node/Test262
gates pass; that result is an enabling baseline, not a speed claim. Existing
tools remain usable with explicit artifact/order control, without overstating
their capabilities. Follow
[benchmark independence](benchmark-integrity.md).
