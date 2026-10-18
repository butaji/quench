# Guarded Emscripten same-memory copy A/B

This is a diagnostic-only, paired 11-round JetStream 3 `gcc-loops-wasm`
comparison of Quench's compiled `_emscripten_memcpy_js` callout with the late
same-memory direct-copy specialization disabled and enabled. Both Quench arms
used the same production binary (`b9260d5c72686f025391c823442d53b99a84e36638151c2331dbea478067fb49`),
the same `--jit` arguments, and differed only in
`QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT=0|1`. Bun 1.4.2 and Node v26.10.0 ran in
every rotated round. All 11 rounds completed with valid status, matching
semantic output, and recorded score/RSS.

| Engine | Median Score | Median peak RSS (MiB) |
| --- | ---: | ---: |
| Quench direct copy off | 51.71 | 48.066 |
| Quench direct copy on | 53.23 | 48.082 |
| Bun JIT | 169.96 | 53.973 |
| Node JIT | 185.59 | 62.399 |

The paired Quench-on/off Score ratio is **1.0425** (95% interval **1.0263 to
1.0750**), a repeatable gain below the predeclared 1.10 keep threshold. The
peak-RSS ratio is **1.0000** (0.99935 to 1.00049), so the change is effectively
RSS-neutral. Quench retains lower RSS than both peers, but its Score is only
0.3143 of Bun's (0.3011 to 0.3315) and 0.2947 of Node's (0.2804 to 0.3045).
This is not a JIT peer win.

A separate one-round `--jit-diagnostics` run confirms the specialization
matched the intended hot path: it recorded **307,300** direct copies among
**308,232** imported callouts in the GCC workload. The direct path's runtime
regression compares overlapping, negative, clamped, and extreme signed indices
against `copyWithin`, both enabled and disabled; it also checks memory growth,
stale and reloaded views, alternate buffers, replaced methods, and callout
cleanup.

The reports are diagnostic and have `qualification_ready: false`. The source
tree was dirty because the implementation and A/B harness were under test; the
within-report comparison is still matched to one Quench executable hash. The
raw reports and analysis are `diagnostic-1-round.json`, `ab-11-round.json`, and
`ab-analysis.json`.

Reproduction:

```sh
target/production/quench-bench --fixtures gcc-loops-wasm --suite jetstream3 \
  --jit --jit-wasm-memcpy-ab --runs 11 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-linux-x64/bun \
  --checkpoint evidence/jit-jetstream3-wasm-2026-10-18/direct-emscripten-copy/ab-11-round.checkpoint.json \
  --out evidence/jit-jetstream3-wasm-2026-10-18/direct-emscripten-copy/ab-11-round.json
target/production/quench-bench --analyze \
  evidence/jit-jetstream3-wasm-2026-10-18/direct-emscripten-copy/ab-11-round.json \
  --out evidence/jit-jetstream3-wasm-2026-10-18/direct-emscripten-copy/ab-analysis.json
```

Decision tracked in Linear TOD-142: reject the late-bound-only optimization as
insufficient and keep its result as a baseline for TOD-143, which moves the
same guarded operation before Wasm-to-JS argument conversion.
