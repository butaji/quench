# CoreMark through `quench-node`, Linux x86_64, 2026-10-17

Decision 7 measures `quench-node`, not the `quench-wasm-test/coremark` harness binary the
earlier CoreMark evidence used. This campaign runs CoreMark through `quench-node` with the
same JS harness Bun and Node run (`run-js.mjs`: `readFileSync`, `WebAssembly.instantiate`,
`env.clock_ms` from `performance.now()`), so the process pays for the Node host it boots.

Host: Linux x86_64 cloud VM, Intel Xeon @ 2.10 GHz, 4 vCPU, 15 GiB, kernel 6.18.44. A shared
guest: scores compare only within one campaign.

## Method

* Bench: CoreMark 1.0, `coremark-minimal-mvp.wasm` from wasm-coremark-rs `0b0e28b1591c`
  (SHA-256 in `binaries.sha256`).
* Quench: the static qualification build of `quench-node` (docs/README.md "Qualification
  builds"), `v2-cloud-wasm` at `85ed4a1ee` for the no-JIT campaign. No link order or profile
  was trained for this run; the binary carries the shipped start-up order, which names no
  scored bench (decision 13).
* Comparators: the wasm-coremark-rs `bm` harness, `cargo build --release -F
  pulley,wamr,wasmtime,winch` plus its defaults, one engine per process. Bun 1.4.2 runs
  `run-js.mjs`; no-JIT is `BUN_JSC_useJIT=0` with `BUN_OPTIONS` unset.
* Each run is its own process under `maxrss.c` (static `fork`/`exec`/`wait4`; 1.25 MB floor
  with `/bin/true`). `run-coremark.sh` runs 11 rounds, every engine once per round, order
  rotated per round; `summarize.py` takes medians.

## No JIT (`coremark-11.csv`, 11 rounds)

| Runtime | Median Score | Range | Median max RSS |
| -- | --: | -- | --: |
| **quench-node** | **3,207.4** | 3,077–3,289 | 20.37 MB |
| Wasmi v2 | 2,896.0 | 2,802–2,989 | 5.65 MB |
| Stitch | 2,224.2 | 2,161–2,335 | 5.33 MB |
| Wasm3 | 2,043.6 | 2,002–2,118 | **4.29 MB** |
| WAMR | 1,885.1 | 1,781–1,978 | 4.38 MB |
| Pulley | 1,463.4 | 1,377–1,506 | 16.43 MB |
| Bun, JIT off | 718.2 | 699–754 | 18.83 MB |
| Tinywasm | 654.9 | 622–690 | 6.29 MB |

* **Score: won on this session.** 1.108x Wasmi v2; ahead in 11 of 11 rounds.
* **Max RSS: lost.** 20.37 MB against Wasm3's 4.29 MB (4.75x).
* Decision 6 needs a second session at least 30 minutes later, and M4 is not measured.

## Where `quench-node`'s CoreMark RSS goes (dynamic production build, median of three)

| Step of the harness | Before (`ed31cf0fa`) | After (`85ed4a1ee`) |
| -- | --: | --: |
| Empty script | 4.49 MB | 4.43–4.61 MB (noise) |
| `require("fs")` | 20.97 MB | 7.03 MB |
| `performance.now()` | 15.85 MB | 4.61 MB |
| `Buffer.allocUnsafe(1)` | 14.62 MB | 13.36 MB |
| CommonJS harness (`r.cjs`: require, readFileSync, run) | 21.45 MB | 15.39 MB |
| ESM `import` of `node:fs` alone | 20.44 MB | 19.62 MB |
| ESM import, `readFileSync`, `new WebAssembly.Module` | 20.32 MB | 21.36 MB |

The static empty process is 4.0 MB. What remains is Node's own library JavaScript, which the
host runs on the shared VM:

1. **The ESM facade reads every export of `node:fs`** (Node's `syncExports` does too), which
   materializes the lazy stream and library groups: about 5 MB.
2. **`readFileSync` returns a Buffer**, which loads Node's `buffer.js` graph (`internal/util/
   inspect`, `internal/errors`, validators, primordials): 12 files, about 341 KB of source,
   about 10 MB resident. Census after it: 53,173 string cells, 4,340 functions, 11,818
   objects; malloc holds the shape table (786 KB), the property arena (524 KB), compiled
   programs (about 1.5 MB) and lane views (342 KB).
3. **The Wasm compile transient.** Inlining grew CoreMark's 3,759 operators to 11,889
   (56 bytes each) and held every body at once (840 KB in `inline_leaf_calls` at the compile
   peak). `85ed4a1ee` inlines each body as it is lowered, with identical output.

The last row of the table is higher after the change: the lower load peak leaves fewer freed
chunks for the Wasm compile to reuse, so its transient lands on fresh pages.

## Instruction guard (`callgrind-guard.txt`)

`evidence/npm-icu-2026-10-16/npm-icu-fixed-callgrind-guard.sh`, dynamic production builds,
base `ed31cf0fa`, candidate `85ed4a1ee`: per-run deltas −0.01% to +0.01% on seven fixtures,
Splay +0.57% (below the 1% hold, decision 24).

## Session 2: no-JIT and JIT (`coremark-session2-11.csv`, 11 rounds)

Static `quench-node` built from `dd7897569` (`v2-cloud-wasm` after merging `v2-merged`; the
published merge `f8c17bb39` adds only this evidence), hash in `binaries-session2.sha256`.
JIT is `quench-node --jit`: the same VM in its compiled mode, Wasm functions through
Cranelift (decision 34). Node is official v26.12.0; Wasmtime and Winch come from the `bm`
harness. Started more than 30 minutes after session 1.

| Mode | Runtime | Median Score | Range | Median max RSS |
| -- | -- | --: | -- | --: |
| No JIT | **quench-node** | **3,086.2** | 2,844–3,236 | 18.14 MB |
| No JIT | Wasmi v2 | 2,935.6 | 2,810–3,015 | 5.38 MB |
| No JIT | Stitch | 2,197.6 | 2,072–2,308 | 4.54 MB |
| No JIT | Wasm3 | 1,996.9 | 1,916–2,098 | **4.04 MB** |
| No JIT | WAMR | 1,871.0 | 1,738–1,981 | 4.15 MB |
| No JIT | Pulley | 1,438.4 | 1,375–1,543 | 14.22 MB |
| No JIT | Bun, JIT off | 690.8 | 670–707 | 18.66 MB |
| No JIT | Tinywasm | 620.5 | 566–672 | 5.43 MB |
| JIT | Node v26.12.0 | **20,898.6** | 19,728–22,036 | 52.86 MB |
| JIT | Bun 1.4.2 | 20,656.9 | 19,939–21,330 | 27.32 MB |
| JIT | Wasmtime | 19,650.2 | 18,397–20,513 | 14.07 MB |
| JIT | **quench-node --jit** | 17,508.0 | 16,694–18,131 | 20.29 MB |
| JIT | Winch | 9,970.1 | 9,361–10,760 | **12.00 MB** |

* **No-JIT Score: won again** (1.05x Wasmi v2, ahead in 10 of 11 rounds). With session 1
  (11 of 11), that is two Linux sessions, but on two binaries (`85ed4a1ee`, `dd7897569`).
* **No-JIT max RSS: lost** (18.14 vs 4.04 MB, 4.5x).
* **JIT Score: lost** (0.84x Node, behind in 11 of 11). **JIT max RSS: lost** (20.29 vs
  Winch 12.00 MB, 1.7x); below Node 52.86 and Bun 27.32 MB, above Wasmtime 14.07 MB.
