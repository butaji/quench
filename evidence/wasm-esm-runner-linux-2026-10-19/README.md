# `quench-node` runs WebAssembly modules as ES modules (Linux x86_64, 2026-10-19)

Intel Xeon @ 2.10 GHz, 4 vCPUs, 15 GiB, Linux 6.18.44, no cgroup limits, rustc 1.99.0.
Node oracle: official v26.12.0.

## Behavior

`quench-node` now runs `.wasm` files the way Node 26 does (`lib/internal/modules/esm/translators.js`
`wasm`, `create_dynamic_module.js`): `import { run } from "./coremark.wasm"` and
`quench-node main.wasm` both work. Import modules are module specifiers, so CoreMark's `env`
import resolves to an `env` package that exports `clock_ms` (`run-coremark-esm.sh` writes it).
The contract is in `docs/node-host-contracts.md`, "WebAssembly modules".

`child_entry` `wasm_files_are_es_modules_as_in_node` runs the same files under Node and
`quench-node` and checks:

* the namespace keys, unwrapped global values, `[object Module]`, a function call and the
  memory;
* a Wasm-to-Wasm global and function import, and a start function that calls a JS import;
* namespace identity on a second dynamic import;
* the `.wasm` entry;
* one `ExperimentalWarning`, sent through `process.emitWarning` as it stands when the warning
  fires;
* `CompileError` with the path prefix, `LinkError` for a missing import, and Node's exact
  `LinkError` messages for reserved import and export names.

Known differences:

* Node rejects a module when it translates it. The facade throws the same error classes, but
  when it evaluates.
* `CompileError` and missing-import `LinkError` messages are the runtime's `WebAssembly`
  messages, not V8's.
* The order of the warning on stderr relative to later stdout differs, as it does for any
  `process.emitWarning` in an ES module under `quench-node` (not specific to Wasm).

## Link order

A first version added `wasmparser` as a direct dependency of `quench-node`. Cargo's metadata
hash covers a crate's dependencies, so every `quench_node` symbol was renamed
(`Cs6hIvQXkVBj` → `Cs7tztoX4Krf4`). The committed static order then missed all of them, and an
empty script's max RSS rose from 3,000 to 3,364 KB (unbolted). The section reader now lives
in `quench-wasm` (`quench_wasm::interface`), which already depends on `wasmparser`. With it,
the hash is unchanged and the empty script measures 2,960 KB.

## CoreMark through the runner path (`runs-unbolted-3.txt`)

Static production `quench-node`, not bolted, evicted before each run, three rounds, max RSS
under `maxrss.c`. Medians:

| Run | Score | Max RSS |
| --- | ---: | ---: |
| `quench-node run-esm.mjs` | 2,738 | 9,000 KB |
| `quench-node --no-warnings run-esm.mjs` | 2,817 | 7,056 KB |
| `quench-node run-js.mjs` (instantiate API) | 2,847 | 4,184 KB |
| `quench-node --jit run-esm.mjs` | 19,998 | 10,772 KB |
| `quench-node --jit run-js.mjs` | 20,111 | 7,416 KB |
| `node run-esm.mjs` | 19,395 | 52,840 KB |
| `node run-js.mjs` | 19,541 | 53,700 KB |

The scores of the two paths match. Max RSS on the ESM path is driven by Node's experimental
warning:

* Reading `process.emitWarning` loads Node's own `internal/process/warning.js` with
  `primordials`, `errors`, `assert`, `options`, `validators`, `util` and `util/types`. That
  adds about 3.5 MB to an empty script.
* Printing the warning goes through `console.error`, which loads the SDK console, another
  2–3 MB.

Both are general host costs that any first warning pays; the follow-up targets them.
