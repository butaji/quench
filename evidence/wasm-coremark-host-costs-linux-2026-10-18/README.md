# What CoreMark's harness costs `quench-node` before the module runs (2026-10-18)

Linux x86_64 cloud VM, 4 vCPU, 16 GB. Static production `quench-node` (qualification flags in
`docs/README.md`), Node v26.12.0 as the oracle. Harness: `run-js.mjs` from
`evidence/wasm-coremark-quench-node-linux-2026-10-17/` (`import { readFileSync } from
"node:fs"`, `WebAssembly.instantiate`, `performance.now`, `console.log` of a string).

## Where the harness's memory went

Empty script 3.9 MB. With the module's bytes inlined into the script (no `node:fs`), CoreMark
runs at 6.1 MB no-JIT and 9.4 MB `--jit`: the rest of the harness's 16.5 / 18.9 MB was Node's
library loaded for `node:fs` and `readFileSync`.

| Step | Cause | Max RSS of the harness through instantiation |
| -- | -- | --: |
| base (`v2-merged` 30b82ee9b) | | 16.46 MB |
| Wasm inlining streamed (`418e6f697`) | operator vectors at compile | 15.80–15.93 MB |
| lazy builtin ESM exports | `import ... from "node:fs"` read every export, building the fs streams graph and Node's fs library | 12.07 MB |
| `readFileSync(path)` without options | ran Node's option policy (`internal/fs/utils.js`) | 11.05 MB |
| string paths | `toPathIfFileURL` loaded Node's URL module (and querystring, webidl, DOMException) to learn a string is no URL | 10.28 MB |

Full run, same binary: no-JIT 10.27 MB (Score 3,373), `--jit` 12.94 MB (Score 18,495), from
16.48 / 18.92 MB.

## The changes and why they keep Node's behavior

**Builtin ESM exports bound lazily.** Node's facade (`syncExports`) binds each export to the
module's value when the facade is made. A member that is still a pending lazy data property
now binds to its pending value instead; the member keeps a listener, and when its group
materializes (by any read) the listener gives the binding the member's value. While a member
has listeners, script's writes and deletes materialize the group first, so the binding keeps
the value the member had, as Node's snapshot does. Namespace reads and named imports resolve
a pending binding as they read it, so script never observes the pending value. Regression:
`child_entry` `builtin_esm_exports_keep_node_snapshots_of_lazy_members` (identity,
descriptors, re-exports, a hot read loop, write/delete/assign after import, import after
materialization), against Node.

**`readFileSync(path)` without options.** Node's `getOptions(undefined | null, defaults)`
returns the defaults, which pass `validateReadFileBufferOptions`. For a primitive path, while
`fs.openSync`, `fs.readSync` and `fs.closeSync` are still the builtins (Node's
`readFileSync` calls them through the module object), the file is opened `O_RDONLY`, read up to
its `fstat` size into `Buffer.allocUnsafe(size)` (Node's pooled allocation), and cut with
`slice` when it shrank. Anything else (options, a descriptor, a URL or Buffer path, a file of
unknown size, a replaced member) keeps Node's general path. `read-file-parity.cjs` compares
both with Node: errors (ENOENT, EISDIR, EBADF, null bytes), pool offsets around the read,
replaced `openSync`/`closeSync`/`Buffer.allocUnsafe` observing their calls. Outputs differ
only where every process differs (`/proc/self/stat`) and where the first pooled allocation
lands in a pool Node's bootstrap has already used.

**String paths.** Node's `isURL` reads `href` first; a string finds it only on
`String.prototype` or `Object.prototype`. When [[HasProperty]] finds none there (it runs no
script), a string is returned as it is without Node's URL module; otherwise Node's
`toPathIfFileURL` runs as before (a `String.prototype.href` getter is still read once).

## Checks

Node `test/parallel` manifest, base and candidate built from the same tree: 813 cases, identical
outcomes except `test-http-server.js` (fails on base, times out on the candidate). `child_entry`:
the same 156 failures on both (the Node 22 oracle on PATH); the candidate adds the new passing
test. Runtime lib 718/718. All-eight guard (`callgrind-guard.txt`, base `6131430f6`, candidate
this commit): per-run deltas within ±0.01%.
