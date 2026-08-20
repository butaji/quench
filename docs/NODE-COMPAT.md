# Quench-node ↔ Node.js compatibility matrix

Target: match Bun's documented Node.js compatibility level
([bun.com/docs/runtime/nodejs-compat](https://bun.com/docs/runtime/nodejs-compat),
vs Node v26). Bun status emoji are reproduced from that page; "Quench" column
is ground truth from source (`crates/quench-node/src/modules/require.rs`,
`registry.rs`, `host.rs`) and focused runs.

Legend
- Bun: 🟢 fully implemented · 🟡 partial (behavior matches Bun doc) · 🔴 not implemented
- Quench: **real** substantive implementation · **partial** exports exist, gaps remain
  · **stub** shape only · **missing** not resolvable / empty namespace

## Node modules

| Module | Bun | Quench | Notes |
|---|:--:|---|---|
| assert | 🟢 | real | 40/40 compat + upstream selects green |
| buffer | 🟢 | real | upstream selects green |
| console | 🟢 | real | upstream selects green |
| dgram | 🟢 | stub | createSocket cap only; needs dgram socket impl |
| dns | 🟢 | partial | lookup + resolve4 only |
| events | 🟢 | real | upstream selects green |
| fs | 🟢 | real | upstream selects green; edge cases remain |
| http | 🟢 | real | server/client/host; options semantics remain |
| os | 🟢 | real | upstream selects green |
| path | 🟢 | real | posix/win32/glob; upstream selects green |
| punycode | 🟢 | missing | not resolvable |
| querystring | 🟢 | real | upstream selects green |
| readline | 🟢 | partial | createInterface only |
| stream | 🟢 | partial | Readable/Writable/Duplex/Transform; web+pipeline partial |
| string_decoder | 🟢 | real | upstream selects green |
| timers | 🟢 | real | upstream selects green |
| tty | 🟢 | real | isatty only |
| url | 🟢 | real | upstream selects green |
| zlib | 🟢 | real | sync flate2; async missing |
| http2 | 🟢 | missing | not implemented |
| sqlite | 🟢 | missing | not implemented |
| trace_events | 🟢 | missing | empty namespace |
| quic | 🟢 | missing | not implemented |
| diagnostics_channel | 🟡 | missing | not resolvable |
| https | 🟡 | partial | request/get caps only; matches Bun partial gaps |
| async_hooks | 🟡 | stub | minimal JS factory |
| child_process | 🟡 | partial | spawn/spawnSync/exec/execSync |
| cluster | 🟡 | missing | empty namespace |
| crypto | 🟡 | missing | not resolvable |
| domain | 🟡 | missing | not resolvable |
| module | 🟡 | partial | CJS require; ESM APIs missing |
| perf_hooks | 🟡 | partial | performance cap only |
| process | 🟡 | partial | properties + basic; signals/lifecycle gaps |
| util | 🟡 | real | upstream selects green; missing diff, transferableAbort* |
| tls | 🟡 | stub | empty namespace (Bun also partial) |
| v8 | 🟡 | missing | not resolvable |
| vm | 🟡 | partial | runInNewContext; modules/options partial |
| wasi | 🟡 | stub | empty namespace |
| worker_threads | 🟡 | stub | Worker cap + isMainThread |
| inspector | 🟡 | stub | empty namespace |
| repl | 🟡 | stub | empty namespace |
| test | 🟡 | stub | minimal sync test runner |
| sea | 🔴 | stub | isSea cap (Bun: not implemented) |

## Globals

| Global | Bun | Quench | Notes |
|---|:--:|---|---|
| console / process | 🟢/🟡 | real | as above |
| Buffer | 🟢 | real | |
| setTimeout/setInterval/setImmediate + clear* | 🟢 | real | |
| queueMicrotask | 🟢 | real | |
| require, module, exports, __dirname, __filename, global | 🟢 | real | CJS |
| atob / btoa | 🟢 | real | |
| structuredClone | 🟢 | real | |
| AbortController / AbortSignal | 🟢 | real | |
| Event / EventTarget | 🟢 | real | EventTarget bound |
| fetch | 🟢 | real (bound) | needs protocol coverage |
| URL | 🟢 | real | |
| TextDecoder / TextEncoder | 🟢 | real | |
| WebAssembly | 🟢 | intrinsic | |
| URLSearchParams | 🟢 | via module only | no global binding |
| Blob, FormData, Headers, Request, Response, MessageChannel/Port, BroadcastChannel, CompressionStream, DecompressionStream, ReadableStream{,BYOB}, WritableStream, TransformStream + controllers/readers/strategies, TextDecoder/EncoderStream, SubtleCrypto/Crypto/CryptoKey, CustomEvent, DOMException, performance, Performance* | 🟢/🟡 | missing | not bound as globals |

## Verification evidence

- `cargo run -p quench-node-test --bin run-compat` — 40 passed, 0 failed.
- `cargo run -p quench-node-test --bin run-parallel` — 177 passed, 0 failed
  (manifest: `crates/quench-node-test/node-tests/parallel.txt`).
- Express smoke app runs under `quench-node`: `curl` → `HTTP/1.1 200 OK`.
- Upstream submodules (`tests/node`, `tests/test262`, `tests/typescript`)
  are byte-for-byte preserved; the harness never rewrites Node's `common`
  helper or fixtures.