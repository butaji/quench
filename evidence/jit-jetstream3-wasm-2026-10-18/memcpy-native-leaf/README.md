# GCC-Wasm native Emscripten memcpy leaf screen

This diagnostic screens a dispatcher-free native leaf for the exact
`_emscripten_memcpy_js(i32, i32, i32) -> ()` import. Both Quench arms enable the
same direct, raw-i32, and early-import paths; only
`QUENCH_WASM_JIT_EMSCRIPTEN_MEMCPY_LEAF` differs. The leaf is guarded by the
existing live-view, method, and unshared-memory checks on the slow path; any
ordinary import or reference callout revokes its raw-memory certificate.

The corrected three-round GCC-Wasm screen produced valid samples and equal
outputs:

| Engine | Median Score | Median peak RSS |
| --- | ---: | ---: |
| Quench, leaf off | 51.20 | 50,102,272 bytes |
| Quench, leaf on | 50.02 | 50,110,464 bytes |
| Bun 1.4.3 | 172.94 | 57,597,952 bytes |
| Node v26.12.0 | 177.67 | 96,419,840 bytes |

The paired leaf-on/off Score ratio was 0.97695 (95% interval 0.94375–1.02385);
the peak-RSS ratio was 0.99984 (0.99829–1.00016). Both intervals include a
tie, so the screen establishes neither a speedup nor a regression. It is not
qualification evidence. Quench retains the lower RSS on this fixture, but its
leaf-on Score is 0.289 of Bun and 0.278 of Node.

An instrumented one-round run, with the same import paths enabled in both
wrappers, counted 307,300 early memcpy imports and **zero leaf hits** in the
leaf-on arm. The summed compiled code grew from 569,416 to 570,696 bytes, but
the specialized path was not reached. More call-pattern instrumentation is
needed to distinguish cache-scope limits from a call-site or guard mismatch.
Do not expand this implementation to eleven rounds until a safe certificate
reaches actual hot calls. Keep it as an opt-in experiment, not a default
optimization.

The earlier `confounded-screen-3-round.json` compared different import paths:
the leaf-on arm also selected early import dispatch while the leaf-off arm did
not. Treat that report and its paired analysis as invalid for measuring the
leaf's effect; they are retained only to document the harness correction.

Reproduce the corrected paired screen with:

```sh
target/production/quench-bench --fixtures gcc-loops-wasm --suite jetstream3 \
  --jit --jit-wasm-memcpy-leaf-ab --runs 3 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-v1.4.3/bun-linux-x64/bun \
  --checkpoint evidence/jit-jetstream3-wasm-2026-10-18/memcpy-native-leaf/isolated-screen-3-round.checkpoint.json \
  --out evidence/jit-jetstream3-wasm-2026-10-18/memcpy-native-leaf/isolated-screen-3-round.json
target/production/quench-bench --analyze \
  evidence/jit-jetstream3-wasm-2026-10-18/memcpy-native-leaf/isolated-screen-3-round.json \
  --out evidence/jit-jetstream3-wasm-2026-10-18/memcpy-native-leaf/isolated-screen-3-round-analysis.json
```

The one-round mechanism reports use `--jit-diagnostics` and
`isolated-quench-with-leaf*.sh`. They confirm the path was compiled but not
entered; they are not performance comparisons. The shorter `quench-with-leaf*.sh`
names preserve the wrappers used by the earlier confounded diagnostic reports.
