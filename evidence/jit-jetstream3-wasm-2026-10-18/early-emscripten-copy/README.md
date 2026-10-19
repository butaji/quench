# Early Emscripten memcpy import A/B

This diagnostic tests TOD-145 on JetStream 3 `gcc-loops-wasm`. Both Quench
arms use the same production executable, `--jit`, raw-i32 memcpy, and the late
direct-copy path; the only difference is
`QUENCH_WASM_EMSCRIPTEN_MEMCPY_EARLY_IMPORT=0|1`. Bun 1.4.2 and Node v26.10.0
run in the same rotated rounds. The three-round screen is diagnostic and is
permanently non-qualifying.

| Engine | Median Score | Median peak RSS (MiB) |
| --- | ---: | ---: |
| Quench early import off | 56.48 | 48.070 |
| Quench early import on | 50.51 | 48.086 |
| Bun JIT | 183.46 | 56.000 |
| Node JIT | 182.24 | 79.660 |

The paired Quench Score ratio (early on/off) is **0.9527** (95% interval
**0.8943 to 0.9661**), showing a regression. Peak-RSS ratio is **1.0004**
(0.9996 to 1.0027), effectively unchanged. The candidate scores 0.2816 of
Bun's and 0.2953 of Node's; its RSS remains lower than both. It misses the
predeclared requirement of at least a 5% paired Score gain with the interval
above a tie and no more than 0.5 MiB RSS growth. The 11-round confirmation was
therefore not run. Keep the switch disabled by default and do not count it as a
JIT or peer win.

The screen used source revision
`472fb5f601f10e8fca18c61456a58bf863b34fc1`, a clean tree, and production
executable SHA-256
`712c99dd41c2d0fca227377705b6d08276dcd125fb198166318c9d263f06ae69`. A
separate one-round diagnostic with a temporary wrapper and the same two
feature switches enabled recorded **307,300 early-import hits** among **308,232
compiled import callouts**. This confirms the measured fast lane is exercised;
the diagnostic build includes only the added hit counter, has a dirty source
tree, and its timing is not used. The paired samples are in
`early-import-screen-3.json`, bootstrap analysis in
`early-import-screen-3-analysis.json`, and path-coverage report in
`early-import-diagnostic-counter-1.json`.

Reproduce the three-round screen with:

```sh
target/production/quench-bench --fixtures gcc-loops-wasm --suite jetstream3 \
  --jit --jit-wasm-memcpy-early-ab --runs 3 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-linux-x64/bun \
  --checkpoint evidence/jit-jetstream3-wasm-2026-10-18/early-emscripten-copy/early-import-screen-3.checkpoint.json \
  --out evidence/jit-jetstream3-wasm-2026-10-18/early-emscripten-copy/early-import-screen-3.json
target/production/quench-bench --analyze \
  evidence/jit-jetstream3-wasm-2026-10-18/early-emscripten-copy/early-import-screen-3.json \
  --out evidence/jit-jetstream3-wasm-2026-10-18/early-emscripten-copy/early-import-screen-3-analysis.json
```

The experiment is tracked in Linear TOD-145. The early lane is a bounded
partial evaluation of known import identity around dynamic Wasm arguments and
memory state, but this case shows that its added checks cost more than the
generic path they skip. The result argues against further per-call import
specialization here. QuickSort's native hot loop and GCC's fixed-trip
arithmetic loops remain separate, higher-upside code-generation targets.
