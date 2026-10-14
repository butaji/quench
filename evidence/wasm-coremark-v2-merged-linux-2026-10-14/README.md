# CoreMark on `v2-merged`: Quench against the top Wasm runtimes, no-JIT and JIT

Date: 2026-10-14 (UTC). Source: `v2-merged` at `f71a0ee24`. Host: Linux x86_64
cloud VM, Intel Xeon @ 2.10 GHz, 4 vCPU, 15 GiB, kernel 6.18.44, rustc 1.99.0.
A shared guest: scores compare only within one campaign.

## Method

* Bench: CoreMark 1.0, `coremark-minimal-mvp.wasm` from wasm-coremark-rs
  `0b0e28b1591c` (module SHA-256 `9fcda04480553b4e…`), host contract
  `env.clock_ms`, export `run` (score, higher is better).
* Comparators: the wasm-coremark-rs `bm` harness built with
  `--features wasmtime,winch,pulley,wamr` plus its defaults (Wasmi v2, Wasmi v1,
  Stitch, Wasm3, Tinywasm), one engine per process (`bm <engine>`). Node v26.10.0
  and Bun 1.4.2 run the module through `run-js.mjs` with the same contract.
  Bun no-JIT is `BUN_JSC_useJIT=0`; `BUN_OPTIONS` is unset so Bun runs stock.
  Node `--jitless` has no `WebAssembly`, so Node is a JIT bar only.
* Quench: `target/production/coremark -no-jit|-jit` from `quench-wasm-test`.
  Builds: `dyn-order` (default production, dynamic C runtime, shipped link
  order), `static-order` (the documented static qualification build, shipped
  static order), `static-noorder` (static, no symbol order), `static-retrained`
  (static, order retrained on this source; see below).
* Each run is its own process under `maxrss.c` (static `fork`/`exec`/`wait4`,
  1.25 MB floor measured with `/bin/true`), giving `ru_maxrss` and wall time.
  11 rounds, every engine once per round, order rotated each round
  (`run-coremark.sh`). Medians and per-round wins: `analyze.py`.
* Binaries: `binaries.sha256`.

## Results (medians of 11 rounds)

Campaign 1, all runtimes (`coremark-11.csv`, `summary-all.txt`):

| Mode | Quench best build | Score bar | RSS bar |
| -- | -- | -- | -- |
| No JIT | Score 2,952 to 2,982 (1.07x Wasmi v2 2,769; ahead 11/11 rounds) | Wasmi v2 2,769 | Wasm3 4.16 MB; Quench 6.99 MB static, 8.07 MB dynamic: **behind** |
| JIT | Score 19,156 to 19,481 (0.96 to 0.97x Node v26 20,040; behind 11/11) | Node v26 20,040; Bun 19,507 | Winch 13.32 MB; Quench 9.48 MB static: **ahead** (Bun 27.2, Node 55.3, Wasmtime 15.0) |

Campaign 2, retrained static order against the bar engines
(`coremark-retrained-11.csv`, `summary-retrained.txt`):

| Mode | Quench `static-retrained` | Bars | Verdict |
| -- | -- | -- | -- |
| No JIT | Score 2,975 (ahead 11/11), max RSS **4.00 MB** | Wasmi v2 2,764; Wasm3 4.12 MB, WAMR 4.16 MB | **Both bars won on this host** (RSS by 3%) |
| JIT | Score 19,311 (ahead 1/11), max RSS **7.12 MB** | Node v26 19,887; Winch 13.20 MB, Bun 27.1 MB | RSS won (0.54x the lowest bar); Score 0.971x Node: **behind** |

## The RSS regression and its cause

The Wasm lane's own branch (`v2-cloud-wasm` `2e4495934`, static order) measures
3.64 MB no-JIT and 6.80 MB JIT on a fixed-work run. Its merge into `v2-merged`
(`f76795545`) measures 6.47 MB; `f71a0ee24` 6.84 MB / 9.58 MB. The extra memory
is resident code: the text segment's RSS is 5.2 MB against 2.1 MB, clean file
pages. The shipped static order names the Wasm lane's symbols; only 1,154 of the
1,728 functions this source executes appear in it, so executed code no longer
shares pages. Retraining the order on this source (cachegrind of CoreMark
`-no-jit` then `-jit` with `COREMARK_CLOCK_STEPS_MS=1,1,2000`, then
`symbol-order`, as docs/README.md prescribes) brings the fixed-work run to
3.83 MB / 6.90 MB. The retrained order is `retrained-x86_64-linux-static.order`;
it is not installed in `crates/quench-wasm-test/link/`.

The retrained order is trained on CoreMark itself; the user allowed
profile-guided layout on 2026-10-14 ("PGO is ok").

## Not established here

* M4 numbers; the Linux two-session rule (decision 6); a qualification run.
* Any JS (V8-v7) result.
