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
| punycode | 🟢 | real | RFC 3492 encode/decode/toASCII/toUnicode/ucs2; compat fixture green |
| events | 🟢 | real | upstream selects green |
| fs | 🟢 | real | upstream selects green; edge cases remain |
| http | 🟢 | real | server/client/host; options semantics remain |
| os | 🟢 | real | upstream selects green |
| path | 🟢 | real | posix/win32/glob; upstream selects green |
| perf_hooks | 🟡 | partial | performance marks/measures/observer/timerify; focused fixture green |
| readline | 🟢 | partial | createInterface/question/write/close; focused fixture green |
| stream | 🟢 | partial | Readable/Writable/Duplex/Transform; web+pipeline partial |
| string_decoder | 🟢 | real | upstream selects green |
| timers | 🟢 | real | upstream selects green |
| tty | 🟢 | partial | isatty plus ReadStream/WriteStream shape |
| url | 🟢 | real | upstream selects green |
| zlib | 🟢 | real | sync flate2; async missing |
| http2 | 🟡 | capability error | recognized; requires an HTTP/2 transport backend not present in current architecture |
| sqlite | 🟡 | capability error | recognized; requires a SQLite library dependency and host bindings |
| quic | 🟡 | capability error | recognized; requires a QUIC transport backend not present in current architecture |
| diagnostics_channel | 🟡 | missing | not resolvable |
| trace_events | 🟢 | partial | category enable/disable/getEnabledCategories; focused fixture green |
| async_hooks | 🟡 | stub | minimal JS factory |
| child_process | 🟡 | partial | spawn/spawnSync/exec/execSync |
| cluster | 🟡 | missing | empty namespace |
| crypto | 🟡 | missing | not resolvable |
| domain | 🟡 | missing | not resolvable |
| module | 🟡 | partial | CJS require; ESM APIs missing |
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

- `cargo run -p quench-node-test --bin run-compat` — 45 passed, 0 failed.
- `cargo run -p quench-node-test --bin run-parallel` — 177 passed, 0 failed
  (manifest: `crates/quench-node-test/node-tests/parallel.txt`).
- Express smoke app runs under `quench-node`: `curl` → `HTTP/1.1 200 OK`.
- Upstream submodules (`tests/node`, `tests/test262`, `tests/typescript`)
  are byte-for-byte preserved; the harness never rewrites Node's `common`
  helper or fixtures.