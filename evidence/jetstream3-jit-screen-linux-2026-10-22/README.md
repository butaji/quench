# JetStream 3 with JIT on: first screen at HEAD (2026-10-22)

Host: Linux x86_64, 4 vCPU, 15 GiB (cloud container); one round per test per engine, one
test per process, stock `cli.js` under the shell prelude, `--no-prefetch`. Quench at
`64da6ab8` (`v2-cloud-jet`) with `--jit` (`qjit`) and without (`qn-evalreg`); Bun 1.4.2 with
`BUN_JSC_useJIT=1` (`jscjit`); Node v26.10.0 as shipped (`v8jit`). Raw rows:
`scores-64da6ab8.txt`; geomeans: `summary.txt`. A screen: one sample, with other work on
the host during part of the run (it can only understate quench).

- Score geomean: quench `--jit` = 0.099 of Bun-JIT, 0.125 of Node-JIT; quench's own
  interpreter = 0.109 of Bun-JIT. `--jit` is 0.907 of the interpreter overall and slower than
  it on 15 tests (sqlite3-wasm 0.17, HashSet-wasm 0.40, tsf-wasm 0.62, json-stringify 0.73,
  hash-map 0.74, async-fs 0.76, typescript-lib 0.77). Faster on gcc-loops-wasm 2.08x,
  quicksort-wasm 1.93x, json-parse 1.25x.
- Max RSS geomean: quench `--jit` = 0.367 of Bun-JIT, 0.262 of Node-JIT; lower than both on
  33 of 34 tests.

First consequence for the JIT track (TOD-110 M0): `--jit` must never be slower than the
interpreter it falls back to; today that alone costs ~10% of the JIT-mode geomean.
