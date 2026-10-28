# Test262 and Wasm testsuite, Quench vs Node vs Bun, no-JIT and JIT

Host: Linux x86_64 cloud container, 4 vCPU Intel Xeon @ 2.10GHz, 15 GiB RAM, 2026-10-22.
Quench: `v2-merged` at `9d4f16725` plus the measurement tools in `304a28fcc` (no runtime
change), **release** build (`cargo build --release -p quench-test262 -p quench-wasm-test`).
Node v26.12.1 (latest stable). Bun 1.4.2, run without the shell's `BUN_OPTIONS=--smol`.

## Method

`quench-bench` `conformance-compare` runs every case as its own process on each engine, as
Quench's own runners do, 4 jobs, (case, engine) pairs interleaved so contention falls on all
engines alike. Each process starts through `rss-exec`, so peak RSS is the process's own
(Linux carries the exec'ing address space's high-water mark into `ru_maxrss`, so a child of
the driver would otherwise report at least the driver's peak). CPU is user+sys from `wait4`.

| | Quench | Node | Bun |
|---|---|---|---|
| Test262 case | `run-all --case [--jit] PATH` | `node [--jitless] tools/test262-engine-case.cjs` | `bun tools/test262-engine-case.cjs` |
| Wasm script | `run --worker FILE` (`QUENCH_EXECUTION_MODE`) | `node [--wasm-jitless] tools/wast-engine-run.cjs` | `bun tools/wast-engine-run.cjs` |
| no-JIT | interpreter | `--jitless` (Test262), `--wasm-jitless` (Wasm: `--jitless` removes WebAssembly) | `BUN_JSC_useJIT=0` |
| JIT | compiled execution | default | `BUN_JSC_useJIT=1` |

Node and Bun run Test262 with the Node oracle's harness composition
(`tools/lib/test262-js-host.cjs`). The 825 module cases are excluded on every engine: that
host cannot load them. Test262 is one round per mode (52,581 cases; per-area sums average
over many cases). Wasm is five rounds per mode, each script's per-engine median
(`--rounds 5`): one round per script is noisy at a few milliseconds.

Commands:

```sh
cargo build --profile iteration -p quench-bench --bin conformance-compare --bin rss-exec
cargo build --profile iteration -p quench-wasm-test --bin wast-json
target/iteration/wast-json WAST_JSON $(find crates/quench-wasm-test/testsuite -name '*.wast')
target/iteration/conformance-compare --suite test262 --mode no-jit|jit --jobs 4 --out DIR \
  --quench target/release/run-all --node NODE --bun BUN
target/iteration/conformance-compare --suite wasm --mode no-jit|jit --rounds 5 --jobs 4 \
  --wasm-json WAST_JSON --out DIR --quench target/release/run --node NODE --bun BUN
```

## Results

| run | engine | passed | CPU s | wall s | RSS median MiB | RSS p95 | RSS max |
|---|---|---|---|---|---|---|---|
| Test262 no-JIT | quench | 52,528 | 768.6 | 846.1 | 12.1 | 13.7 | 1,696 |
| | node | 51,284 | 2,120.2 | 2,343.4 | 41.4 | 43.3 | 1,556 |
| | bun | 51,738 | 960.9 | 1,033.2 | 21.2 | 25.6 | 4,247 |
| Test262 JIT | quench | 52,528 | 763.2 | 857.2 | 12.2 | 13.8 | 1,698 |
| | node | 51,284 | 2,055.3 | 2,264.1 | 45.6 | 47.4 | 1,560 |
| | bun | 51,732 | 950.7 | 1,035.1 | 21.9 | 27.4 | 2,241 |
| Wasm no-JIT (288 scripts) | quench | 288 | 6.4 | 6.8 | 10.2 | 14.9 | 147 |
| | node | 249 | 10.1 | 11.2 | 43.6 | 50.7 | 62 |
| | bun | 220 | 4.0 | 4.6 | 21.2 | 24.3 | 31 |
| Wasm JIT | quench | 288 | 7.3 | 7.9 | 12.6 | 16.3 | 70 |
| | node | 249 | 10.0 | 11.3 | 43.6 | 50.8 | 62 |
| | bun | 220 | 4.2 | 4.9 | 22.4 | 31.9 | 39 |

Quench's 53 Test262 non-passes are the Node-waived cases. Per-case medians, Test262:
Quench 7.4 ms CPU / 12.1 MiB, Bun 12.0 ms / 21.2 MiB, Node 32.3 ms / 41.4 MiB (no-JIT; JIT
alike). Quench's ratio, CPU: 0.36 of Node and 0.80 of Bun on Test262; 0.64 of Node and
1.61 of Bun on Wasm (no-JIT). Median RSS: 0.23-0.29 of Node and 0.48-0.57 of Bun everywhere.

The suite totals sit at the margin against Bun because a few areas cost seconds per case;
the median case wins by about 40%. `test262-areas.md` lists every Test262 area short of the
20% margin; each `summary.md` lists the areas per run.

## Where Quench is not 20% ahead (cause cases, CPU ms Quench / Node / Bun)

- `language/identifiers` 3.3x Node, 7.5x Bun: `start-unicode-*-class.js` 2,400-3,800 / 50 / 50-66.
- `built-ins/String`: `prototype/repeat/empty-string-returns-empty.js` 10,000 / 37 / 11.
- `built-ins/TypedArray` RSS max 22x Node: `prototype/copyWithin/coerced-values-{start,end}-detached*.js`
  11,000-12,000 ms and 1.7 GB / 160 ms 53 MB / 260 ms 36 MB.
- `built-ins/Array` RSS max 1.6x Node: `15.4.4.{16..20}-*-c-ii-*` (sparse length near 2^32)
  1,000 ms 92 MB / 120 ms 41 MB / 90 ms 21 MB.
- `built-ins/Atomics` 3.7x Node: `waitAsync/**/no-spurious-wakeup-*.js` 2,000 ms CPU each
  (Node and Bun fail these at once without `$262.agent`; Quench spends its wait on CPU).
- `staging/sm` Date `dst-offset-caching-*-of-8.js` 4,900 / 3,300 / 500-1,450.
- `built-ins/decodeURI*` 2.2-3.2x Bun: `S15.1.3.1_A2.5_T1.js` 5,600 / 11,900-31,200 / 660-1,500.
- `language/comments` 2.6x Bun: `S7.4_A5.js`, `S7.4_A6.js` 1,700-1,900 ms 150 MB / 1,100-2,400 / 420-480.
- `built-ins/RegExp` 1.3x Bun no-JIT, 2.2x Bun JIT: property escapes 350-440 / 200-570 / 90-170.
- Near the margin vs Bun: `annexB/built-ins` 0.83, `ArrayBuffer` 0.84, `parseInt` 0.84-0.90,
  `parseFloat` 0.82, `encodeURI*` 1.2-1.5.
- JIT: Quench's compiled execution saves nothing on Test262 (763 vs 768 CPU s) and costs on
  Wasm (7.3 vs 6.4 CPU s, median RSS +2.4 MiB); `br_table.wast` 385 ms JIT vs 84 ms no-JIT.
- Wasm: `return_call.wast`, `return_call_ref.wast` 540-620 / 38-47 / 23-217;
  `memory_copy*.wast` 180-230 / 60 / 40-75; `table_copy*.wast` 130 / 55 / 31;
  `skip-stack-guard-page.wast` 147 MB no-JIT RSS / 44 / 26.

## Caveats

- Quench's Wasm worker parses `.wast` text inside the measured process; Node and Bun read
  scripts `wast-json` already encoded (all 288 convert in 4.5 s on an iteration build).
- The JS API cannot carry v128 values or results: Node and Bun skip those commands
  (`unsupported`), so the SIMD scripts (`simd_*.wast`, Quench's largest Wasm costs after
  tail calls) are not comparable, and Node's and Bun's Wasm pass counts are lower partly
  for that reason.
- The Test262 per-case processes measure startup plus a small case: they rank engines on
  startup-dominated work, not on long-running throughput (V8-v7, JetStream).
- Single host, one Test262 round; per-case wall times share 4 vCPUs with 3 other jobs.
