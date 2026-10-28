# CoreMark through `quench-node`'s Wasm ES module runner: warnings and release points (Linux x86_64, 2026-10-19)

Intel Xeon @ 2.10 GHz, 4 vCPUs, 15 GiB, Linux 6.18.44, no cgroup limits, rustc 1.99.0. Static
production `quench-node` (docs/README.md "Qualification builds"), CoreMark
(`coremark-minimal-mvp.wasm`) through
`../wasm-esm-runner-linux-2026-10-19/run-coremark-esm.sh`: `run-esm.mjs` imports `run` from
the Wasm module (Node's Wasm ESM integration), whose `env` import resolves to a package
exporting `clock_ms`; the same binaries also run `run-js.mjs` (`WebAssembly.instantiate`).
Max RSS is `ru_maxrss` under that campaign's `maxrss.c`; every binary is evicted before
each run.

## What the runner path paid for

The first runner build (`../wasm-esm-runner-linux-2026-10-19`, unbolted) measured 9.0 MB
max RSS without the JIT against 4.2 MB for the instantiate-API harness on the same binary:

* Node's experimental warning. Reading `process.emitWarning` loaded Node's
  `internal/process/warning.js` through the library loader, with `internal/errors`,
  `internal/validators`, `internal/options`, `internal/assert`, the primordials,
  `internal/util` and `internal/util/types` (3.5 MB over an empty script); printing it went
  through `console.error`, which loads the SDK console (another 2–3 MB). Node prints the
  same warning, so the runner cannot skip it. `process.release`, which the listener reads,
  shared a lazy group whose script cost about 580 KB.
* Pages touched between release points. Loading the module graph, the warning, and
  instantiating stayed resident under the Wasm compiler's peak and the JIT's: the last
  release before them was the entry's compilation.

## Change

* **Warnings** (`process_warning_shared_vm`). `process.emitWarning` and the default
  listener's implementation are Node's `emitWarning` and `onWarning` from
  `internal/process/warning.js`, which runs unchanged (exact source text and positions)
  with its own `require` and `primordials`: the primordials it uses from the startup
  intrinsics, `getOptionValue` over the parsed options, a `validateString`, `assert` and
  `ERR_INVALID_ARG_TYPE` that load Node's own modules only on the paths that throw (with the
  stacks Node's give), `path.basename`, the public builtins, and the console's `error`.
  `--expose-internals` keeps the library module.
* **Console** (`console_shared_vm::warning_writer`). `writeOut` reads the global console's
  `error` when it first writes, as in Node. While that member is still the unloaded SDK
  console's, it is a writer that writes the line to `process.stderr` as the SDK's `error`
  writes one string, and calls the SDK's own `error` once the SDK console loads (group
  indentation); a replaced `console.error` is used as Node uses it.
* **`process.release`** is its own lazy group with a native initializer. Its key moves from
  after `debugPort` to before `getActiveResourcesInfo`; Quench's `process` key order already
  differed from Node's, where `release` follows `platform`.
* **Release points.** `NodeHost::compile_wasm` also releases the read-only pages before it
  decodes, and a new runtime host hook, `Host::before_wasm_native_compilation`, runs once per
  Wasm program before the JIT compiles it (its first call); `quench-node` releases there.
  The first facade instantiation releases the module graph's pages, then builds the
  warning's functions before the module compiles.
* **Facade instantiation** is native (`esm_wasm::instantiate`): no helper script to compile
  during evaluation. Unchanged behavior; its `WebAssembly` constructors and the instance
  registry (a `WeakMap`) are captured once over the startup intrinsics.

## Progression (unbolted static builds, max RSS KB, no JIT / JIT)

| Build | `run-esm.mjs` | `run-js.mjs` |
| --- | --- | --- |
| first runner build | 9,000 / 10,772 | 4,184 / 7,416 |
| warning fast path, `process.release` group | 4,636 / 9,088 | 4,252 / 7,412 |
| + release before decoding | 4,500 / 8,220 | 4,012 / 7,420 |
| + release before the JIT compiles | 4,500 / 6,120 | 4,012 / 6,408 |
| + release after loading the graph | 3,868 / 6,212 | 3,968 / 6,448 |

These rows used a hand-written equivalent of Node's warning functions. Node's module run
verbatim costs about 0.4 MB more unbolted (compiling its text faults more of the compiler
back in) and 0.1–0.2 MB bolted; it keeps `process.emitWarning`'s source and stacks Node's
(docs/node-host-contracts.md, "Public warning construction"), so it is the one committed.

## Cost of the release points

JetStream 3 Wasm fixtures, one iteration with compilation
(`../wasm-jit-loops-and-addresses-linux-2026-10-18/jetstream-fixed.cjs`), wall ms, five
interleaved runs, medians, this change against the same tree without the two new release
points (unbolted):

| Fixture | `--jit` | no JIT |
| --- | --- | --- |
| zlib | 767 / 767 | 652 / 664 |
| quicksort | 399 / 383 | 496 / 517 |
| gcc-loops | 880 / 849 | 814 / 917 |

Each release costs one `madvise` and the refaults of the hot pages (about 2 ms); the
differences are within this host's run-to-run variation.

## Behavior

* `child_entry` `process_warnings_match_node_without_loading_the_warning_module_graph`:
  `emitWarning` forms (string, type, code, options object, Error, deprecation, constructor,
  empty type), validation errors, listener payloads, a replaced `console.error`,
  `console.group` indentation, a custom `toString`, under no flags, `--no-warnings`,
  `--disable-warning` and `--no-deprecation`: stdout and stderr identical to Node v26.12.0.
  `--redirect-warnings` writes the same file. `--trace-warnings` and `--throw-deprecation`
  differ from Node only by Node's internal frames, as before; against the previous build the
  one change is a replaced `console.error`, which the previous build bypassed and Node uses.
* `process.emitWarning.toString()` is Node's source; validation errors carry
  `node:internal/process/warning:157:5` frames, as before.
* `child_entry` `wasm_files_are_es_modules_as_in_node` passes.

## Campaign (`coremark-esm-11.csv`, 11 rounds)

`../wasm-esm-runner-linux-2026-10-19/run-coremark-esm.sh` with every engine; static
`quench-node` of this change, bolted from Callgrind run profiles of that same binary (the
documented corpus; `bin/quench-node` in `binaries.sha256`). The `-esm` rows run
`run-esm.mjs`, the others `run-js.mjs` or the wasm-coremark-rs `bm` harness. Max RSS in MiB.

| Mode | Runtime | Median Score | Range | Median max RSS |
| -- | -- | --: | -- | --: |
| No JIT | **quench-node, Wasm ESM** | **2,900.9** | 2,788–2,956 | **3.68 MB** |
| No JIT | quench-node, `run-js.mjs` | 2,899.6 | 2,808–3,010 | 3.41 MB |
| No JIT | Wasmi v2 | 2,699.8 | 2,447–2,739 | 5.07 MB |
| No JIT | Stitch | 2,007.1 | 1,928–2,024 | 4.23 MB |
| No JIT | Wasm3 | 1,846.8 | 1,781–1,874 | 3.82 MB |
| No JIT | WAMR | 1,741.6 | 1,674–1,776 | 4.04 MB |
| JIT | **quench-node --jit, Wasm ESM** | **20,455.5** | 19,227–21,011 | **6.03 MB** |
| JIT | quench-node --jit, `run-js.mjs` | 20,442.9 | 18,839–21,115 | 6.21 MB |
| JIT | Node v26.12.0, `run-js.mjs` | 19,216.0 | 18,763–20,161 | 52.42 MB |
| JIT | Node v26.12.0, Wasm ESM | 19,107.1 | 18,048–19,898 | 51.74 MB |
| JIT | Wasmtime | 18,086.5 | 17,305–18,374 | 13.43 MB |
| JIT | Winch | 9,477.0 | 8,627–9,689 | 11.55 MB |

With `quench-node` as the Wasm runner (Wasm ESM rows):

* **No-JIT Score: won** (1.074x Wasmi v2, ahead in 11 of 11 rounds).
* **No-JIT max RSS: won** (3,704–3,836 KB against Wasm3's 3,840–4,024 KB, below it in 11 of
  11 rounds).
* **JIT Score: won** (1.065x Node's best row, ahead of every engine in 10 of 11 rounds).
* **JIT max RSS: won** (6.03 MB against Winch's 11.55 MB, 11 of 11).

The instantiate-API rows improved as well (no JIT 3.67 → 3.41 MB, JIT 6.91 → 6.21 MB against
`../wasm-jit-inline-small-modules-linux-2026-10-19`), from the two Wasm release points.

The committed tree also drops one host binding left unused by this change (a function
table entry); that build, bolted from the same profiles, is checked below.
That build (`bin/quench-node-committed`), three evicted runs: Wasm ESM no JIT 3,756 KB in each
(Score 2,887–2,968), `--jit` 6,120–6,164 KB (Score 19,829–20,958).

## Checks

* `child_entry`: the two tests above pass. A full run (483 cases, Node v26.12.0 first on
  `PATH`) taken while bolting and RSS runs shared the host failed 49; the six not in the
  earlier list (`async_hooks_keep_active_snapshot…`, `canceled_preconnect_write…`,
  `child_identity_uses_kernel_credentials…`, `deep_comparison_modes…`,
  `exec_file_matches_node…`, `exec_normalization_and_collection…`) pass when rerun alone.
  `process_default_warning_listener_preserves_sdk_source…` fails before and after: the
  bootstrap's stand-in listener's source is not Node's.
* `node_contracts` and `shared_regressions` pass; `quench-node` lib 23/23; runtime lib
  751/751.
