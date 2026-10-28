# CoreMark max RSS: a link order that applies, lazy `fs`, split Wasm compile release (Linux x86_64, 2026-10-19)

Intel Xeon @ 2.10 GHz, 4 vCPUs, 15 GiB, Linux 6.18.44, no cgroup limits; rustc 1.99.1 for the
retrained-order rows, 1.99.0 for the committed build.
Static production `quench-node` (docs/README.md "Qualification builds"), CoreMark through
`evidence/wasm-coremark-quench-node-linux-2026-10-17/run-js.mjs` on `coremark-minimal-mvp.wasm`,
each run under that campaign's `maxrss.c`. Every binary is evicted from the page cache before
it is measured (`posix_fadvise(POSIX_FADV_DONTNEED)`, docs/README.md); see "Page cache" below.

## The committed static order matched almost nothing

`link/x86_64-linux-static.order` names Rust symbols with their crates' hashes
(`Cs1j3G5KV77xm_14quench_runtime`). A build on this host names them
`Cs21TTOm9UD5K_14quench_runtime`: of the 1,658 functions the CoreMark harness faults in, 45
were in the order. Cargo's metadata hash includes the full `rustc -vV`, and
`rust-toolchain.toml` named the channel `1.99`, which resolves to whichever 1.99 patch release
an environment installed: the evidence tree records both 1.99.0 and 1.99.1 on x86-64 Linux.
The order was trained under one and linked under the other, so the layout was the default
one and the RSS gains it was measured with were absent.

`rust-toolchain.toml` now names `1.99.0` exactly: the release the RSS lane trains the order
with (its b38 order, merged here, names `Cs1j3G5KV77xm`). Built with 1.99.0 on this host, the
merged tree names the same hashes, and 5,411 of the order's 5,492 symbols are present.

## Retrained static order

The measurements below up to the lazy-`fs` row were taken under rustc 1.99.1 with orders
retrained here; the committed order is the RSS lane's b38 order (trained under 1.99.0), which
the exact pin makes apply on both hosts. Both were trained with the documented procedure (`evidence/tod123-rss-linux-2026-10-14/README.md`, "Run-phase
layout"): Cachegrind and Callgrind (`--dump-before=quench_entry_compiled`) profiles of the
empty start, `quench-bench/deegen-curriculum`, `quench-bench/micros`, the 300-file Test262
sample and the Wasm kernels; `symbol-order --dense 13dispatch_fast --foreign-first` with the
run tiers. Profiles are named by their directory (`curriculum-001.js.out`,
`micros-001.js.out`): with plain basenames the two directories' `001.js` … overwrite each
other. No scored bench is in the corpus (decision 13). BOLT (`bolt-20`, the documented
`bolt-profile` and `llvm-bolt` flags) is applied from the same profiles.

## Lazy `fs`

`import { readFileSync } from "node:fs"` built the whole module: about 50 guest factories
compiled and called, `fs.promises`, `Stats`, the sync and callback wrappers. That was 1.26 MB
of the harness's anonymous memory and about 2.5 MB of start-up code touched before the Wasm
module ran. `fs_shared_vm::module` now defines `readFileSync` and makes every other member
one lazy data property group (`MODULE_MEMBERS`, in the module's existing key order, with
each member's enumerability): `initialize_module` runs the existing builder and the library
policy install when a script first observes any member, then checks that the module's own
keys are exactly `readFileSync` followed by `MODULE_MEMBERS`. `readFileSync` checks that
`openSync`, `readSync` and `closeSync` are still Node's own; a member still pending counts as
its own (`is_pending_lazy_data_property_rooted`, new), since assignment replaces a pending
member without materializing it. `globalThis.__nodeFs` stays eager.

The only key-order change: `readFileSync` is `fs`'s first own key instead of the second
(Quench's order already differs from Node's; Node lists 105 keys, Quench 97). Descriptors,
values and identities are those of the eager module (`import * as`, named and default
imports, `fs.promises === require("fs/promises")`, `Object.freeze`, `constants`, `delete`,
replacing `openSync` before `readFileSync`: identical output to the previous build).

Callgrind instructions (GLIBC tunables as docs/README.md):

| Script | Eager | Lazy |
| --- | ---: | ---: |
| empty ESM entry | 49.7M | 50.2M |
| `import fs from "node:fs"` | 194.5M | 84.5M |
| every `fs` member's descriptor (`Reflect.ownKeys` + `getOwnPropertyDescriptor`) | 1,702.5M | 1,705.7M |

## Wasm compile releases between its phases

Decoding with validation and lowering touched 2.8 MB of code between the release points
around them, the harness's peak once `fs` was lazy. `NodeHost::compile_wasm` now releases
the read-only pages after `Engine::compile` too, not only after lowering.

## Results

Max RSS, KB (`ru_maxrss`), three evicted runs each:

| Build | No JIT | `--jit` | Empty script |
| --- | ---: | ---: | ---: |
| base `0f510ab21` (order not applied) | 6,112 | 7,640–7,800 | 3,828 |
| retrained order | 5,516 | 6,644 | 2,856 |
| retrained order + BOLT | 4,996–5,052 | 6,376 | 2,356 |
| + lazy `fs`, order retrained on it, + BOLT | 4,024–4,032 | 5,288 | 2,380 |
| + split Wasm compile release (this change) + BOLT | 3,740–3,780 | 5,268–5,280 | |

Comparators in the same session (`bm`, evicted): Wasm3 3,896–4,156 KB (no-JIT score
1,821–1,845), WAMR 4,152–4,180, Wasmi 5,328–5,428; the 2026-10-17 JIT comparators were
Winch 12.00 MB and Wasmtime 14.07 MB. No-JIT scores of the builds above stay within
2,756–2,929 (Wasmi 2,652–2,690 today).

## Campaign (`coremark-11.csv`, 11 rounds)

`run-coremark-evicted.sh`: the 2026-10-17 campaign with each engine's binary evicted before
each of its runs, one engine per process under `maxrss.c`, order rotated per round. Static
`quench-node` from `d4beb8dfe` (the merged tree, rustc 1.99.0, the b38 order) bolted from this
corpus's Callgrind run profiles (hash in `binaries.sha256`); Node is official v26.12.0;
the other engines are the wasm-coremark-rs `bm` harness.

| Mode | Runtime | Median Score | Range | Median max RSS |
| -- | -- | --: | -- | --: |
| No JIT | **quench-node** | **2,867.2** | 2,790–2,935 | **3.58 MB** |
| No JIT | Wasmi v2 | 2,691.4 | 2,604–2,771 | 5.05 MB |
| No JIT | Stitch | 2,005.2 | 1,896–2,039 | 4.30 MB |
| No JIT | Wasm3 | 1,824.9 | 1,772–1,846 | 3.86 MB |
| No JIT | WAMR | 1,718.0 | 1,687–1,764 | 4.05 MB |
| JIT | Node v26.12.0 | **19,541.4** | 18,981–19,843 | 52.32 MB |
| JIT | **quench-node --jit** | 18,728.9 | 18,255–19,525 | **5.23 MB** |
| JIT | Wasmtime | 18,135.7 | 17,869–18,516 | 13.33 MB |
| JIT | Winch | 9,306.3 | 8,440–9,681 | 11.67 MB |

* **No-JIT Score: won** (1.065x Wasmi v2, ahead in 11 of 11 rounds).
* **No-JIT max RSS: won** (3,664 KB in every round; Wasm3 3,860–4,068 KB, below it in 11 of 11).
* **JIT max RSS: won** (5.23 MB against Winch 11.67 MB, 11 of 11).
* **JIT Score: lost** (0.958x Node, ahead in 1 of 11); ahead of Wasmtime (1.033x).

## Page cache

A binary that `llvm-bolt` (or any large sequential write) just wrote sits in the page cache
as 2 MB folios, and a read fault in a 2 MB-aligned stretch of it maps the whole folio
(`FilePmdMapped`), whatever userfaultfd registration the process made: the bolted binary
measured 17.6 MB with 8 MB of its read-only segment mapped by four PMDs, and 4.0 MB once
evicted. Measure only evicted binaries.

## Checks

* Runtime lib 739/739.
* `quench-node-test` with Node v26.12.0 first on `PATH`: `child_entry` fails 44 of 481 in one
  full run against the base's 38. Of the six that differ, four pass when rerun alone
  (`child_launches_inherit_guest_cwd...`, `exec_reports_kill_override_errors...`,
  `ipc_process_counted_references...`, `legacy_pipe_close_and_throwing_completion...`);
  `global_crypto_is_installed_before_commonjs_entry_and_preloads` finishes at its child
  deadline on either tree (`evidence/wasm-coremark-release-after-load-linux-2026-10-18`);
  `native_buffer_pool_shares_real_aligned_backing...` timed out once in three reruns, taking
  14.2–15.2 s against the base's 14.4–14.8 s with a 15 s deadline, and its output matches
  Node's when run directly. `shared_regressions`' `vm_context_lexicals_survive_collection...`
  fails on the base too. `node_contracts`, `worker_threads` and `napi_addons` pass.
