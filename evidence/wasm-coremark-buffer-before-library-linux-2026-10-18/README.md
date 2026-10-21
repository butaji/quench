# Buffers before `buffer.js` loads (Linux x86_64, 2026-10-18)

CoreMark's harness reads its module with `readFileSync(path)`, which returns a Buffer, and hands
the bytes to `WebAssembly.instantiate`, which reads them through the view's internal slots.
Nothing looks at a Buffer method, yet making the Buffer loaded Node's `buffer.js` graph (12
files, 341 KB of source: `internal/util/inspect`, `internal/errors`, validators, primordials).
Compiling those sources alone, without running them, cost 3.3 MB of max RSS; touching the
`Buffer` global cost 4.6 MB.

## Change

A Buffer is a `FastBuffer` (`internal/buffer.js`: `class FastBuffer extends Uint8Array {}`)
whose prototype `buffer.js` fills in. The host now makes that class itself, from the same
statement at the same line under the startup intrinsics, and gives its prototype
`Buffer.prototype`'s 97 own keys as pending members of one lazy group:

- `constructor` stays in place for `buffer.js` to assign; every other member is *redefined*:
  materialization removes it before the initializer (loading `buffer.js`) runs, so the
  library defines it with its own attributes (`parent` and `offset` are non-configurable
  accessors) at its own position.
- Script's writes, deletes and definitions of a member, listing the prototype's keys with
  its symbols, and fixing its integrity materialize the group first, so script never observes
  a placeholder; reads materialize it and return the library's value with the right receiver.
- `internal/buffer.js` takes the host's class (`require('internal/quench/fast_buffer')`, an
  internal request only Node's library can make) instead of declaring its own, so the
  prototype's identity is one before and after the load, whichever loads first (Node's
  library loads `internal/buffer.js` without `buffer.js` for streams and encodings too).

Until `buffer.js` loads, `readFileSync(path)` allocates as `Buffer.allocUnsafe(size)` would
(`allocate`): below half of `Buffer.poolSize` (64 KiB) from a pool made like `createPool()`
(an untransferable ArrayBuffer of 64 KiB plus 64 bytes, its FastBuffer 64-byte aligned,
slices 8-byte aligned), otherwise a FastBuffer of its own (on the heap up to 64 bytes). When
`buffer.js` reaches its top-level `createPool()`, it adopts the host's pool state
(`require('internal/quench/buffer_pool')`), so later allocations continue in the same pool as
in Node; from then on only the library allocates. Pool ownership is a named state machine
(`PoolOwner`: unborn, host, library).

The runtime's lazy groups (`lazy_data_property.rs`) gained symbol keys and redefined members
(`LazyMember`), used through `define_lazy_members_rooted`.

## Parity with Node v26.12.0

`parity.cjs` reads a 7,684-byte file (CoreMark's size, pooled), a 100,000-byte file and a
50-byte file before anything touches `Buffer`, then observes them; `node-v26.12.0-expected.tsv`
is Node's output for each case, and Quench's is identical for all 15 (both as
`child_entry` tests: `read_file_before_buffer_keeps_node_buffer_semantics`,
`buffer_prototype_keys_match_node`):

prototype identity with `Buffer.prototype` and `instanceof`; own keys, string keys and symbols
in order; descriptors before and after materialization; pool offsets and sharing with a later
`Buffer.allocUnsafe(10)`; `ArrayBuffer.prototype.transfer` refused on the pool;
`util.inspect` and `toString`; `parent`/`offset`; a write, a delete, a `defineProperty` and an
`Object.freeze` of the prototype before the library; `for...in` over a Buffer; `Buffer`
global first; `require('stream')` first.

Node `test/parallel` manifest (813 cases), base `c26d8b603` and candidate built from the same
tree: identical outcomes except `test-http-server.js`, which fails or times out on either
(timeout then fail on base, fail twice on the candidate). The Buffer, fs-read, Blob,
string-decoder and zlib inventory (238 cases): identical.

## Max RSS

Static production `quench-node`, unbolted, CoreMark harness
(`evidence/wasm-coremark-quench-node-linux-2026-10-17/run-js.mjs`); Intel Xeon @ 2.10 GHz, 4
vCPUs, 15 GiB, Linux 6.18.44, no cgroup limits.

| Run | Before | After |
| --- | ---: | ---: |
| through `readFileSync` | 9,284 KB | 7,224 KB |
| through instantiation | 9,412 KB | 7,996 KB |
| full run, no JIT (3 runs) | 9,708 KB | 7,996 KB |
| full run, `--jit` (3 runs) | 11,592–11,680 KB | 8,656 KB |

Base is `v2-cloud-wasm` before merging `v2-merged` at `c26d8b603`; the empty process measured
3.7 MB there and 4.0 MB at `c26d8b603` itself, with or without this change (a regression the
merge brought in, measured separately).

`--jit` is now below Winch (12.00 MB) and Wasmtime (14.07 MB) from the 2026-10-17 campaign.
No-JIT stays above Wasm3 (4.04 MB): the empty process is 3.7 MB, and `import ... from
"node:fs"` builds `fs` with 55 host JavaScript snippets (2.7 MB, next).
