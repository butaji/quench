# Splay stock Score versus fixed-work counters (M4)

This diagnostic tests whether the stock V8-v7 harness adds disproportionate work to Quench, or whether process-level cycle counts are a poor proxy for stock Score.

Host: Mac16,10, Darwin arm64 T8132, 10 logical CPUs, 16 GiB. Runtime binaries and the fixed-work report are from `ba3eee054`; the instrumented stock source only adds one callback after each existing `Measure` loop to record its already-maintained `n` count. No timing hook runs inside `benchmark.run()`.

## Method

The instrumented stock input uses the ordinary `BenchmarkSuite.RunSuites` flow. It records warmup and measured `run()` counts separately. Its K=0 twin uses that same flow, setup, and teardown, with `RunSingleBenchmark` returning `null` so no warmup or measured loop runs. K=0 and work order alternates per round. For each pair, process counters subtract K=0 and divide by the stock work's warmup-plus-measured call count. The existing fixed-work artifact supplies the 300-run/K=0 comparison. `timing_clean` requires the maximum pre/post one-minute load to stay below 1.0 per logical CPU; all 11 K=0/work pairs were clean for all four engines.

The preceding pilot is retained as `pilot-contended-double-setup-k0.json` for provenance, but is invalid for inference: its load was 2.8–3.3 per logical CPU and its K=0 accidentally ran setup twice.

## Results

Stock Score medians below come from the uninstrumented 11-round stock report. Wall time per run from that Score is `100 × 81,491 / Score`, since Splay's reference is 81,491. Stock-cycle and K=0-subtracted wall values use the new 11-round instrumentation. Fixed-work values use the saved 300-run, 11/11-clean report.

| Engine | Stock Score | Stock Score wall/run | Stock marginal wall/run | Fixed-work marginal wall/run | Stock marginal cycles/run | Fixed-work marginal cycles/run | Stock CPU/wall |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Quench | 6,090 | 1.338 ms | 1.335 ms | 1.337 ms | 5.287 M | 5.269 M | 1.00× |
| Node v26.10.0 `--jitless` | 14,139 | 0.576 ms | 0.585 ms | 0.608 ms | 4.421 M | 4.985 M | 2.04× |
| Bun 1.4.2 no-JIT | 9,624 | 0.847 ms | 0.844 ms | 0.824 ms | 7.059 M | 6.666 M | 2.41× |
| QuickJS 2026-06-04 | 8,442 | 0.965 ms | 0.970 ms | 0.976 ms | 3.845 M | 3.853 M | 1.00× |

Each engine has 11/11 clean pairs. Median stock calls (warmup / measured / total) were Quench 747 / 746 / 1,492; Node 1,723 / 1,729 / 3,449; Bun 1,188 / 1,178 / 2,369; QuickJS 1,029 / 1,033 / 2,061.

## Interpretation

The stock Score speed gap is **2.32×** against Node, and the measured stock marginal wall/run ratio is **2.28×**. Fixed-work marginal wall/run is **2.20×** against Node. These wall measurements agree closely, so the stock `Measure` loop is not supported as a Quench-specific extra cost.

Cycle ratios tell a different story: Quench/Node is **1.20×** for stock K=0-subtracted cycles/run and **1.06×** for fixed-work cycles/run. The earlier 1.13× figure is not reproduced by the saved fixed-work report, which records Node at 4.985 M cycles/run (Quench 5.269 M). In both stock and fixed-work runs, Node's process CPU time is about 2× wall time and Bun's about 2.4×, while Quench and QuickJS are about 1×. The accumulated CPU time shows parallel CPU activity in the reference-engine processes; alongside their cycles-per-wall-second rates, this indicates that `cycles elapsed` is not a critical-path counter. The data do not identify which engine worker threads account for that extra CPU time.

Use stock Score as the qualification speed metric and fixed-work marginal wall/run as the diagnostic proxy. Keep cycles for same-engine attribution and report them as aggregate process work, not as cross-engine speed distance. No stock-driver optimization is indicated by this comparison; focus speed work on reducing elapsed time on Quench's run path.

Raw rows and paired derivations are in `stock-counters.json` and `stock-counters-analysis.json`; exact input hashes and invocation details are embedded there. The runner is `measure_stock_cycles.py`.
