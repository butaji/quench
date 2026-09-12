# 05 — Correctness and performance harness

Status: complete

The harness records benchmark scores, errors, timeouts, engine identity, host metadata, and alternating A/B samples. Long-running benchmarks use bounded process cleanup. Profiling and ranked results are separate from correctness.

Evidence: `scripts/perf-cycle.sh` and `scripts/run-v8v7.sh`.

Known measurement issue: unrelated host CPU load has produced noisy samples. Do not kill unrelated processes; instead use alternating samples and record host conditions.

Timed score runs do not enable per-opcode counters; those counters execute inside the hottest bytecode loop and would measure the profiler. `profile` explicitly enables `DEEGEN_OPCODE_STATS`, keeping observation at the profiling edge.
