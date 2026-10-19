# A module's text is held once (2026-10-18)

Host: `host.txt` (x86_64 Linux 6.18.44 VM, 4 CPUs, 16 GiB RAM, no cgroup limits).
Comparators: Node v26.12.0 and Bun 1.4.2; Bun ran with this environment's
`BUN_OPTIONS=--smol`. Base: v2-npm 69c12a87b (m22 equivalent). Candidate: this commit (m23).

## Finding

`ModuleSource` carried `source` (the text the VM compiles) and `bytes` (the module's bytes)
as two owned buffers. The VM clones a `ModuleSource` into dynamic import jobs, static graph
nodes and deferred namespaces, so each extra buffer was copied too.

- Every ES module held its text twice: `bytes` was `source`'s UTF-8.
- A CommonJS file imported from ESM is compiled through a small interop wrapper. Its
  `bytes` were the whole wrapped file, which no Node API reads: Node rejects
  `with { type: 'bytes' }` (`ERR_IMPORT_ATTRIBUTE_UNSUPPORTED`, checked on the oracle) and
  has source-phase objects only for WebAssembly.
- `await import('typescript')` kept the 9 MB `typescript.js` alive through evaluation in
  several copies.

## Fix

- `ModuleSource::original_bytes` is `Some` only when the bytes are not the source's UTF-8.
  `ModuleSource::bytes()` derives them otherwise.
- `ModuleSource::text` builds a module whose bytes are its text.
- `ModuleSource::from_bytes` decodes a file and keeps the raw bytes only when they are not
  UTF-8. The test262 host uses it, so `type: 'bytes'` of a binary file is unchanged.
- quench-node builds every module with `text`: its interop wrappers no longer keep the file
  they wrap.

## Probes (peak RSS MiB, `tests/npm-top300`)

| probe | base | candidate | Node | Bun |
| --- | --- | --- | --- | --- |
| `await import('typescript')` | 134 | 116.5 | 109 | 62 |
| `require('typescript')` | 109 | 109.5 | 92 | 66.5 |

## npm top 300 (release builds; all 300 pass)

| run | lowest of 300 | median Quench / Node / Bun MiB |
| --- | --- | --- |
| m22 (base) | 283 | 17.3 / 69.3 / 38.5 |
| m23 (candidate) | 283 | 17.3 / 70.4 / 38.0 |

Largest changes: @typescript-eslint/tsconfig-utils 141.6 → 124.2, lucide-react 38.6 → 36.4,
the @babel packages and vite −0.7 to −1.5. The only increases, +0.3 to +1.7, are declaration
checks whose peak is TypeScript 7's native `tsc`, the same Go process for every engine.

## What remains (`rss-report.txt`)

The @typescript-eslint family is about 35–45 MiB above Bun. That comes from loading
`typescript.js` (9 MB) at about 109 MiB:
- **Parse arena: 54.5 MB.** It is about 6× the source, because OXC builds the whole tree
  before compiling. It is freed after the compile, which is when the peak occurs.
- **Compiled program: about 21 MB.** 21,729 function records of 392 B each, plus 6.5 MB of
  code; about 90% of the functions are never called. Compile-on-first-call
  (TOD-21 P-07/P-08) is the lever for this.
- **Export detection:** Node's CJS/ESM syntax detection
  (`commonjs::detect_source_kind`) parses a package file without a `"type"` field once
  more before compiling it. That parse is freed before the compile, so it costs time but
  not peak.

Massif charges the function table's doubling (17 MB) to the compile. Large blocks grow by
`mremap`, though, so the capacity nobody touches is not resident.

## Checks

- The change touches only module loading (`ModuleSource` construction and the `type: 'bytes'`
  read), which the instruction guard's script fixtures never run, so the guard was not rerun.
- quench-runtime lib (iteration): 733 passed.
- quench-node `child_entry` (iteration, oracle first on PATH): 464 passed, 17 failed, all of
  them the known baseline set (five crypto tests the oracle's OpenSSL rejects, brotli/zlib
  streaming, two Buffer, three TextDecoder, private_fields, warning listener,
  tcp_reuse_port, util_text_codec, utility_sdk).
