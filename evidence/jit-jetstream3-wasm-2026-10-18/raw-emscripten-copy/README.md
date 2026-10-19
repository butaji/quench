# Raw-i32 Emscripten memcpy callout A/B

This diagnostic-only experiment measures the early compiled-import path for
JetStream 3 `gcc-loops-wasm`. Both Quench arms used the same production binary
(`205b8514c6075f2b22ac539c3f5146853270b6f8d4aab613e7d784c7ab2f8537`),
the same `--jit` arguments, and
`QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT=1`; the arms differed only in
`QUENCH_WASM_EMSCRIPTEN_MEMCPY_RAW_I32=0|1`. Bun 1.4.2 and Node v26.10.0 ran
in every rotated round. All 11 rounds completed with matching semantic output
and valid score/RSS measurements.

| Engine | Median Score | Median peak RSS (MiB) |
| --- | ---: | ---: |
| Quench raw-i32 off | 53.61 | 48.219 |
| Quench raw-i32 on | 53.82 | 48.211 |
| Bun JIT | 167.64 | 53.910 |
| Node JIT | 185.51 | 62.379 |

The paired Quench-on/off Score ratio is **1.0211** (95% interval **0.9814 to
1.0491**), which includes a tie and falls far below the predeclared 1.25 keep
threshold. The paired peak-RSS ratio is **0.99984** (0.99943 to 1.00041),
effectively unchanged. The candidate is not a peer win: its Score is 0.3177 of
Bun's (0.3074 to 0.3326) and 0.2910 of Node's (0.2828 to 0.3162). It does
retain lower peak RSS than both.

A separate one-round diagnostic recorded **307,300** raw-i32 hits among
**308,232** compiled import callouts; the late W-08 counter was zero. Thus the
candidate exercised its intended path, but bypassing Wasm-to-JS argument
conversion did not provide a statistically supported score improvement on
this workload. Keep it opt-in and disabled by default for further experiments;
do not count it as a JIT win.

The raw report has `qualification_ready: false` and `source_dirty: true` while
the candidate was under development. Raw samples, paired analysis, and the
diagnostic hit report are in `ab-11-round.json`, `ab-analysis.json`, and
`diagnostic-1-round.json`.

Reproduction:

```sh
target/production/quench-bench --fixtures gcc-loops-wasm --suite jetstream3 \
  --jit --jit-wasm-memcpy-raw-ab --runs 11 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-linux-x64/bun \
  --checkpoint evidence/jit-jetstream3-wasm-2026-10-18/raw-emscripten-copy/ab-11-round.checkpoint.json \
  --out evidence/jit-jetstream3-wasm-2026-10-18/raw-emscripten-copy/ab-11-round.json
target/production/quench-bench --analyze \
  evidence/jit-jetstream3-wasm-2026-10-18/raw-emscripten-copy/ab-11-round.json \
  --out evidence/jit-jetstream3-wasm-2026-10-18/raw-emscripten-copy/ab-analysis.json
```

The hit-count diagnostic used a temporary wrapper that enabled both direct-copy
switches around the same production `quench-node`; its raw report records the
wrapper hash and is diagnostic only. The decision is tracked in Linear TOD-143.
