# QuickSort direct comparison-branch A/B

This diagnostic evaluates TOD-144 on JetStream 3 `quicksort-wasm`. The Quench
arms use the same production executable and `--jit` arguments and differ only
in `QUENCH_WASM_JIT_DIRECT_COMPARE_BRANCHES=0|1`. Bun 1.4.2 and Node v26.10.0
run in each rotated round. All 11 rounds completed with valid, matching
QuickSort output. The report remains `qualification_ready: false` by design.

| Engine | Median Score | Median peak RSS (MiB) |
| --- | ---: | ---: |
| Quench direct branches off | 217.46 | 42.852 |
| Quench direct branches on | 211.41 | 42.906 |
| Bun JIT | 357.47 | 47.078 |
| Node JIT | 419.65 | 61.441 |

The paired on/off Score ratio is **0.9693** (95% interval **0.9362 to
1.0238**), and the paired RSS ratio is **1.0031** (0.9947 to 1.0092). Neither
metric establishes an improvement. The candidate Score is 0.5914 of Bun's
(0.4953 to 0.6592) and 0.5045 of Node's (0.4492 to 0.5094); RSS remains lower
than both. Keep the switch disabled by default and do not count this as a JIT
win.

The three-round screen had an on/off median Score ratio of 1.2026, but its 95%
interval (0.9897 to 1.3090) included a tie. This triggered the predeclared
11-round confirmation, which did not confirm a gain. The x64 Cranelift lowering
rules route `brif` through `is_nonzero_cmp`
(`vendor/cranelift-codegen/src/isa/x64/lower.isle:3603`), and that matcher maps
both `icmp` and `uextend(icmp)` to the same `emit_cmp` form
(`vendor/cranelift-codegen/src/isa/x64/inst.isle:3871-3874`). A lower-level
condition alone was therefore not evidence of a machine-code improvement.

The raw report records Quench binary SHA-256
`14f7919b3f571f2ecc708abb620cd79ee8c679bd75236650b38b8fa1a8748279` for both
arms, source revision `bb3d924ac50be782fe8ea2908b535802305148e4`, and a dirty
source tree while this experiment was in development. Quench's JIT and
benchmark package tests passed before the trial.

Reproduction:

```sh
target/production/quench-bench quicksort-wasm --suite jetstream3 --jit \
  --jit-wasm-compare-branch-ab --runs 11 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-linux-x64/bun \
  --out evidence/jit-jetstream3-wasm-2026-10-18/direct-compare-branch/trial-11-round.json
target/production/quench-bench --analyze \
  evidence/jit-jetstream3-wasm-2026-10-18/direct-compare-branch/trial-11-round.json \
  --out evidence/jit-jetstream3-wasm-2026-10-18/direct-compare-branch/trial-11-round-analysis.json
```

The experiment is tracked in Linear TOD-144. Raw 3-round and 11-round reports
and their paired analyses are saved alongside this file.
