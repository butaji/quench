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
| punycode | 🟢 | real | RFC 3492 encode/decode/toASCII/toUnicode/ucs2; compat fixture green |
| dgram | 🟢 | partial | real UDP bind/send/close/address; focused fixture green |
| dns | 🟢 | partial | host lookup/reverse/resolve APIs covered by focused fixtures; remaining API edges |
| events | 🟢 | real | upstream selects green |
| fs | 🟢 | real | upstream selects green; edge cases remain |
| http | 🟢 | real | server/client/host; options semantics remain |
| https | 🟡 | stub | HTTPS-specific request/get/Agent/TLS integration remains |
| os | 🟢 | real | upstream selects green |
| net | 🟢 | partial | loopback sockets and server APIs covered; full option and fd semantics remain |
| path | 🟢 | real | posix/win32/glob; upstream selects green |
| perf_hooks | 🟡 | partial | performance marks/measures/observer/timerify; focused fixture green |
| querystring | 🟢 | real | parser/stringifier surface implemented |
| string_decoder | 🟢 | real | upstream selects green |
| timers | 🟢 | real | upstream selects green |
| tty | 🟢 | partial | isatty plus ReadStream/WriteStream shape |
| url | 🟢 | real | upstream selects green |
| zlib | 🟢 | real | sync flate2; async missing |
| http2 | 🟢 partial | loopback HTTP adapter over existing HTTP host; protocol-specific gaps remain |
| sqlite | 🟢 real partial | bundled rusqlite DatabaseSync memory/open/exec/prepare/run/all/close; focused fixture green |
| diagnostics_channel | 🟡 | partial | bootstrap channel surface now resolves; built-in channel coverage remains |
| trace_events | 🟢 | partial | category enable/disable/getEnabledCategories with overlapping-tracer reference counting; focused fixture green |
| async_hooks | 🟡 | stub | minimal JS factory |
| child_process | 🟡 | partial | spawn/spawnSync/exec/execSync |
| cluster | 🟡 | partial | single-process lifecycle/worker handle; child IPC remains |
| crypto | 🟡 | partial | `randomBytes`, hashes, HMAC, WebCrypto AES-GCM, and cipher validation are present; cipher/key edge coverage remains |
| domain | 🟡 | missing | not resolvable |
| module | 🟡 | partial | CJS require, `builtinModules`, and CLI eval; ESM APIs and `createRequire` integration remain |
| tls | 🟡 | partial | `createSecureContext` returns an opaque context; TLS transport remains |
| util | 🟡 | real | upstream selects green; missing diff, transferableAbort* |
| v8 | 🟡 | missing | not resolvable |
| vm | 🟡 | partial | runInNewContext; modules/options partial |
| wasi | 🟡 | stub | empty namespace |
| worker_threads | 🟡 | partial | synchronous Worker lifecycle plus in-process MessageChannel; asynchronous worker execution remains |
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
| URLSearchParams | 🟢 | real global | wired to native `SPEC_URL_SEARCHPARAMS_NEW` |
| Blob / FormData / Headers / Request / Response | 🟢 | partial | embedded-JS implementations include Request options and GET/HEAD body validation |
| MessageChannel / MessagePort / BroadcastChannel | 🟢 | partial | connected in-process MessageChannel ports and close/start behavior; transfer and worker integration remain |
| CompressionStream / DecompressionStream | 🟢 | missing | unavailable |
| ReadableStream / WritableStream / TransformStream | 🟢 | partial | native web-stream primitives and Node interop remain |
| TextDecoderStream / TextEncoderStream | 🟢 | missing | unavailable |
| SubtleCrypto / Crypto / CryptoKey | 🟢 | partial | common operations exist; algorithm and unsupported-operation coverage remains |
| CustomEvent / DOMException | 🟢 | partial | embedded-JS implementations exist; native edge semantics remain |
| performance / Performance* | 🟢/🟡 | partial | marks/measures exist; resource timing and complete observer behavior remain |

## Verification evidence

Measured after the latest compatibility fixes:

- `cargo run -p quench-node-test --bin run-compat -- --quiet` —
  **68 passed, 0 failed, 68 total**.
- `cargo run -p quench-node-test --bin run-parallel` —
  **178 passed, 0 failed, 178 total**.
- Focused diagnostics_channel, inspector, repl/wasi, DNS, path, events,
  HTTP, net, and readline fixtures pass individually.
- Focused `trace_events` overlap fixture passes: two tracers sharing a category retain it until both disable.

These results cover the current repository manifests. Bun's green/yellow
labels remain reference classifications from the current Bun Node v26 page,
not proof that every Bun-documented API is implemented by Quench.