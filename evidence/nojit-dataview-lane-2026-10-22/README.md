# Interpreter-mode Float64 DataView fast lane experiment

## Question

Does allowing the existing guarded `DataView.getFloat64`/`setFloat64` lane fast path in interpreter mode improve JetStream 3 `async-fs`?

## Method

- Host: Linux x86_64 container, 16 GiB memory cgroup.
- Quench production binary was built from the experiment at `v2-jit` HEAD `410d83929` plus the uncommitted dispatch gate change; SHA-256 `18c380a1a6c9042c2e1bbfba5affcc907f719b0a51bf0d6e2f6b9d60cb2a10bd`.
- The disabled arm used `/tmp/quench-dataview-off`, a wrapper that sets `QUENCH_JS_JIT_DATAVIEW_FLOAT64=0` inside the measured child and then execs that exact binary. The enabled arm invoked the same binary directly (default enabled).
- Each arm used `quench-bench --fixtures async-fs --suite jetstream3 --runs 3 --timeout-ms 600000` with the pinned stock fixture, Bun 1.4.3 with JIT disabled, and Node v24.19.0 with `--jitless`.
- Both reports are valid in all three rounds and output-identical. This is a screening A/B, not an 11-round qualification run.

## Results

| Arm | Quench median score | Quench median max RSS |
| --- | ---: | ---: |
| Fast path disabled | 9.39 | 35.129 MiB |
| Fast path enabled | 9.34 | 35.164 MiB |

The enabled arm changed the median score by -0.5% and RSS by +0.035 MiB. Per-round score variation was substantial (disabled 8.62–9.78; enabled 7.64–12.19). The earlier runs that attempted to toggle the parent environment were invalid controls because the benchmark runner launches measured engines with a clean environment; those raw reports are retained as `off.json` and `on.json` for provenance but must not be used for the decision.

## Disposition

Rejected. The candidate did not improve interpreter-mode `async-fs`; its production gate and test were removed. The existing compiled-mode fast path remains unchanged. This workload-specific result is not a claim about the full no-JIT suite, but does not support retaining this change toward the performance goal.
