# Module surface — register every `builtinModules` entry

> Contract: This task is part of broad Node 24 compatibility across Linux
> x86_64, Linux ARM64, macOS, and Windows. Native addons and Node-API are
> excluded. Use the statuses and release gates in
> [compatibility-contract.md](../docs/compatibility-contract.md).

## Contract alignment

This task supports the Node 24 application-runtime contract on Linux x86_64;
observable Node behavior remains the compatibility target. See
`docs/authoritative-test-sources.md` for the Node, LLRT, Deno, WPT, and Test262
reference roles.

## Goal

Make the module surface a declaration/IR consumed by generated registration and
surface tests; avoid one-off registration code per module.

Every name in `require('module').builtinModules` either has a working polyfill
branch in `crates/quench-node/polyfills/bootstrap.js` or returns the
Node-correct `Error("No such built-in module: …")` with code
`ERR_UNKNOWN_BUILTIN_MODULE`. No `require('node:<x>')` or `require('<x>')`
returns `undefined` or throws a generic JS error.

## Current state

The 32 modules registered in `bootstrap.js` (grouped):

- Core: `assert`, `assert/strict` (via `__nodeAssert`), `buffer`, `cluster`,
  `child_process`, `crypto`, `events`, `fs`, `fs/promises`, `os`, `path`,
  `path/posix`, `path/win32`, `querystring`, `constants`, `stream`,
  `stream/iter`, `stream/promises`, `stream/web`, `stream/consumers`,
  `string_decoder` (TODO), `timers`, `timers/promises`, `url`, `util`,
  `util/types`, `v8`, `vm`, `zlib`, `zlib/iter`.
- Async: `async_hooks`, `perf_hooks`.
- Process: `process` (via `globalThis.process`), `worker_threads` (stub).
- Networking: `http` (in-process), `https` (unsupported TLS boundary), `http2`
  (unsupported boundary), `net` (in-process), `dgram` (in-process), `tls`
  (unsupported boundary), `dns`, `dns/promises`.
- I/O: `readline`, `readline/promises`, `repl`, `tty` (TODO), `console` (via
  `globalThis.console`), `module` (TODO), `diagnostics_channel`, `inspector`
  (return ERR), `wasi` (return ERR), `domain` (legacy).
- Internal: `internal/event_target`, `internal/errors`, `internal/buffer`,
  `internal/test/binding`, `internal/fs/utils`, `internal/modules/*`.

## Backlog (60 modules, alphabetical)

For each entry: status, the focused stage(s) that gate it, the up-stream fixture
cluster, and the next concrete slice.

### Already registered (verify, no new slice)

- `assert` — done; verify Node parity in `tests/node-compat/stage-?/`.
- `assert/strict` — done via `__nodeAssert`.
- `async_hooks` — done (task 006 follow-up).
- `buffer` — done (task 001); gaps in `toString` and `copyBytesFrom` are
  Node-correct.
- `child_process` — minimal (task 006 follow-up). Need: `exec`/`execFile`,
  `execSync`/`execFileSync`, `spawn` argv0, `stdio` array, `fork` IPC channels,
  signal propagation, env merging, stdio pipe, detached, `windowsHide`.
- `cluster` — minimal primary-only (task 009 next slice). Need: full `Worker`
  lifecycle, IPC, env propagation, `setupPrimary` arg variants, `disconnect`
  cleanup.
- `console` — focused core surface is covered by stage 828; `Console`,
  `console.assert`, `console.trace`, `console.table`, and group methods are
  available. The module `dir`/`createTask` exports are covered by stage 2557.
- `constants` — return `{...libuv constants…}` (mostly no-op on rquickjs host).
- `crypto` — hash/HMAC/random/uuid/cert done. Need: `Cipheriv`/`Decipheriv`,
  `Sign`/`Verify`, `KeyObject`, `KeyExportCallback`, `createPrivateKey`,
  `createPublicKey`, `diffieHellman`, `scrypt`, `pbkdf2` (done), `hkdf`,
  `hashStream`, `secretKeyObject`, `webcrypto` (`crypto.subtle`).
- `dgram` — in-process socket shape and bind/send/close behavior are covered by
  stages 554, 2261–2266, 2272, 2285–2298, 2302–2305, and 2340–2341. Real OS UDP
  callbacks (`__quench_udp_socket`/send/recv/close) remain open in task 014.
- `diagnostics_channel` — stage 530 complete: channels, subscriptions, stores,
  and tracing channels. No host work.
- `dns` / `dns/promises` — partial (`__nodeDns.resolve*`). Need: real
  `dns.lookup`, real `lookupService`, `Resolver`, `setServers`, `getServers`,
  `setLocalAddress`, `cancel`, `getDefaultResultOrder`. Host: real
  `__quench_dns_lookup`.
- `domain` — stage 531 provides the legacy `Domain` lifecycle and member
  management API; error-event integration remains target-specific.
- `events` — done (EventEmitter).
- `fs` / `fs/promises` — partial. Need: `fs.opendir`, `fs.cp`, `fs.statfs`,
  `fs.lutimes`, `fs.lchmod`, `fs.lchown`, `fs.copyFile` flags, `fs.constants`,
  `FileHandle.readLines` (async iterator), `fs.promises.glob`, real watcher.
- `http` — in-process only. Need: real `http.request`/`http.get` via host TCP,
  `http.Agent`, `http.IncomingMessage`, `http.ServerResponse`, chunked transfer,
  `expect-continue`, `100 Continue`, `flushHeaders`, `setTimeout`, `keepAlive`,
  `http.ClientRequest` abort.
- `https` — TODO. Stage-515: `https.request` over real TLS via host.
- `http2` — TODO. Stage-516: `http2.createServer` minimal (in-process loopback).
- `inspector` — return `ERR_UNKNOWN_BUILTIN_MODULE`.
- `module` — stage 529 provides `builtinModules`, `isBuiltin`, `createRequire`,
  `_cache`, and `_extensions`.
- `net` — minimal. Need: real `net.createServer`/`net.connect` via host TCP;
  `net.Socket`, `net.BlockList`, `net.isIP`, `net.isIPv4`, `net.isIPv6`.
- `os` — partial. Need: real `os.cpus`, `os.networkInterfaces`, `os.userInfo`,
  `os.setPriority`, `os.getPriority`, `os.homedir`, `os.tmpdir`.
- `path` / `path/posix` / `path/win32` — done (task 009).
- `perf_hooks` — done (task 003 follow-up). Need: `PerformanceObserver` gc/stat
  callback, `monitorEventLoopDelay`.
- `process` — partial. Need: `process.report`, `process.dlopen`,
  `process.setUncaughtExceptionCaptureCallback`, `process.chdir` (real via
  host), `process.kill`, `process.exit` (real via host), real `process.uptime`,
  real `process.memoryUsage`, real `process.cpuUsage`.
- `punycode` — stage 528 complete: Unicode domain conversion and UCS-2 helpers
  use a compact JavaScript Bootstring implementation.
- `querystring` — done.
- `readline` / `readline/promises` — partial; stage 532 adds the promise
  `Interface` and `createInterface` surface. Need: `Interface` class,
  `createInterface`, `emitKeypressEvents`, real keypress decoding via host
  `__quench_tty_*`.
- `repl` — stage 533 provides an in-process `repl.start()` server with
  evaluation, prompt, event, and close contracts.
- `stream` — done (task 004). Need: `pipeline`, `finished`, `Readable.from`
  (done), `Writable.duplex`, `Transform._transform`, `PassThrough`.
- `stream/iter` — gated by `--experimental-stream-iter`. Done.
- `stream/promises` — stage 525 complete: promise-based `pipeline` and
  `finished` reuse the existing evented stream contracts.
- `stream/consumers` — stage 527 provides promise consumers for buffered bytes,
  text, JSON, ArrayBuffer, and Blob values.
- `stream/web` — stage 526 registers minimal WHATWG
  `ReadableStream`/`WritableStream`/`TransformStream` primitives.
- `string_decoder` — stage 519 complete: UTF-8 `StringDecoder.write()` and
  `end()` preserve incomplete multibyte sequences across Buffer chunks. Stage
  2575 additionally covers Latin-1, ASCII, and UTF-16LE decoding without relying
  on unsupported host `TextDecoder` labels. The full upstream fixture still has
  an environment/coverage failure and remains open for follow-up.
- `sys` — alias to `util`.
- `timers` / `timers/promises` — done (task 003).
- `tls` — stage 520 registers the TLS surface and returns an explicit
  `ERR_TLS_NOT_SUPPORTED` error for network operations until the planned host
  OpenSSL/rustls boundary is available.
- `trace_events` — return `ERR_UNKNOWN_BUILTIN_MODULE`.
- `tty` — stage 521 registers `ReadStream`, `WriteStream`, `isatty`, color, and
  window-size APIs; non-TTY behavior is explicit until the termios host boundary
  is implemented.
- `url` / `URL` / `URLSearchParams` — done. Need: `URL.canParse`, `URL.parse`,
  `url.fileURLToPath` (real), `url.pathToFileURL` (real).
- `util` — partial. Need: `util.promisify`, `util.types.*`, `util.styleText`,
  `util.parseArgs`, `util.transferableAbortSignal`. `styleText` now follows
  Node's `(format, text, options)` signature, validates styles, supports
  aliases/background styles, and is covered by stage 541; nested restoration in
  the full upstream fixture remains open.
- `util/types` — stage 524 complete: register the standard ArrayBuffer,
  typed-array, collection, promise, RegExp, and Date predicates.
- `v8` — done. Need: `v8.writeHeapSnapshot`, `v8.getHeapStatistics`,
  `v8.stopCoverage`, `v8.takeCoverage`.
- `vm` — partial. Need: `vm.SourceTextModule` (ESM), `vm.SyntheticModule`,
  `vm.Module` linker, `vm.compileFunction`, real `vm.createContext` shared array
  buffer.
- `wasi` — return `ERR_UNKNOWN_BUILTIN_MODULE`.
- `worker_threads` — stub. Need: real `worker_threads` via host `std::thread`.
  Out of scope for the in-process simulator: track as "unsupported on this
  target" with `ERR_WORKER_NOT_SUPPORTED`.
- `zlib` — stage 522 complete: `createGzip`, `createGunzip`, `createDeflate`,
  and `createInflate` provide evented in-process transforms.
- `zlib/iter` — stage 523 complete: async iterable gzip and gunzip transforms
  reuse the zlib synchronous byte primitives.

## Slicing rules

- One focused stage per `Stage N: <feature>` row in this file.
- One commit per stage, per the project workflow.
- The slice order is the one in this file (top to bottom), so reviewers can read
  `tasks/011` and see the planned order.
- Slices marked "real host work" include the matching host call contract in
  `tasks/014`.

## Done when

- `grep 'if (name ===' crates/quench-node/polyfills/bootstrap.js | wc -l` ≥ 60.
- For each entry above, either the corresponding stage passes (focused) or the
  entry returns the documented error code under a `node:disable`-style skip.

## Status

Stage 519 is complete. The implementation required no new host callback: the
polyfill keeps incomplete UTF-8 bytes locally and delegates complete sequences
to the existing `TextDecoder`. The focused stage caught that rquickjs does not
flush `TextDecoder.decode(undefined)`, so `end()` explicitly supplies an empty
typed array. Future slices should include a direct runtime probe for boundary
inputs before finalizing the focused assertion. Stage 520 is complete as a
surface slice. It exposes secure-context metadata and a stable unsupported error
for connection/server operations; the real TLS host boundary remains tracked
separately rather than hidden behind a generic missing-module error. Stage 522
reuses the existing synchronous compression primitives through a small evented
adapter, avoiding another Rust callback while preserving stream `data`/`end` and
`pipe` behavior for the focused contract. Stage 523 applies the same reuse
strategy to async iterables; the focused round-trip test confirms chunk
boundaries are preserved as iterable values without adding a host callback.
Stage 524 confirms `util/types` can be implemented entirely from JavaScript
intrinsics, with no host callback or Rust surface expansion. Stage 525 confirms
promise stream orchestration can remain in JavaScript: `pipeline`/`finished`
subscribe to the existing `pipe`, `end`, and `error` events without expanding
the Rust host. Stage 526 found rquickjs does not provide WHATWG stream globals,
so the focused module uses a small queue/controller implementation in
JavaScript; no Rust host state is required for the initial contract. Stage 527
reuses the same reader contract and centralizes collection in one helper,
keeping byte conversion behavior consistent across all consumers. Stage 528
confirmed rquickjs URL hostname normalization does not provide Punycode
conversion, so the module uses the RFC 3492 algorithm directly and keeps the
implementation independent of a Rust or external runtime hook. Stage 529 reuses
the existing global loader for `createRequire`, keeping the module registration
surface in JavaScript without introducing a second Rust module system. Stage 530
keeps diagnostics state in a module-level channel map, so repeated
`channel(name)` calls share subscribers without requiring Rust-side globals.
Stage 531 keeps the legacy domain implementation deliberately small and
state-based; it adds no host exception machinery while preserving lifecycle and
callback binding contracts. Stage 532 layers promise questions over the existing
line-event convention; the focused test uses a tiny input double, keeping
terminal I/O out of the module slice until the tracked TTY callbacks are
implemented. Stage 533 keeps REPL evaluation inside the JS runtime and exposes
the server callback contract without adding a separate Rust evaluator or
terminal loop. Stage 534 centralizes the common file, signal, and copy constants
in a frozen JavaScript object, matching the existing host/polyfill numeric
contracts. Stage 535 confirms `assert/strict` can be an exact alias of the
existing assertion implementation, avoiding duplicated assertion logic and
preserving strict comparison behavior. Stage 536 confirms the legacy `sys`
module is best represented as the exact `util` object alias, preserving shared
formatting and inspection behavior. Stage 537 converts the unsupported
`trace_events` path into an explicit `ERR_UNKNOWN_BUILTIN_MODULE` error, making
unsupported-target behavior Node-compatible and testable instead of leaking a
generic loader exception. Stage 538 applies the same explicit unsupported-module
contract to `wasi`; WASI runtime integration remains outside the in-process
rquickjs target. Stage 539 extends the explicit unsupported boundary to both
inspector module variants, preventing debugger APIs from falling through to a
generic loader error on this target. Stage 540 adds `util.parseArgs` for
boolean/string options, negated flags, positionals, repeated values, and token
reporting without expanding Rust. Stage 541 adds `util.styleText` with common
ANSI styles, nested style arrays, and color-disable options while keeping
formatting entirely in JavaScript. Stage 542 adds `util.callbackify`, including
callback validation, success delivery, and synchronous exception delivery
through the existing job queue. Stage 543 adds `util.transferableAbortSignal`
and `util.transferableAbortController` with signal validation, marking, and
abort propagation without requiring structured-clone host support. Stage 544
adds the `console` module constructor and table, tracing, grouping, and
assertion methods by delegating output to the existing process streams. Stage
545 adds `URL.canParse` and `URL.parse`, including relative URL bases and
null-returning invalid input behavior where rquickjs lacks the statics. Stage
546 adds V8 heap-statistics and coverage controls with stable numeric shapes;
heap snapshots return an explicit unsupported-target error. Stage 547 adds
`os.homedir`, `os.tmpdir`, and `os.userInfo` using the existing host-provided
process directories and a stable identity object shape. Stage 548 adds
`process.uptime`, `memoryUsage`, and `cpuUsage` with stable Node-compatible
result shapes while the deeper host accounting remains a separate optimization
slice. Stage 549 adds a frozen `fs.constants` object for open and copy flags,
reusing the numeric contract already exposed by the general constants module.
Stage 550 adds `stream.PassThrough` with evented data forwarding, end handling,
and piping while reusing the existing in-process stream model. Stage 551 adds
`process.report` metadata, `getReport`, and report flags with an explicit
JavaScript-runtime report shape and no native dump dependency. Stage 552 adds
the common single-directory wildcard form of `fs.promises.glob` as an async
generator over the existing directory primitive. Stage 553 registers `dns` and
`dns/promises` with server configuration, resolver, callback lookup, and promise
lookup contracts; real DNS resolution remains a subsequent host-boundary slice.
Stage 554 registers the `dgram` UDP socket shape with bind/send/close events and
address reporting; actual datagram I/O remains tracked for the Rust socket
boundary. Stage 555 registers `https` with explicit TLS-unsupported errors for
request, get, and server operations until the planned OpenSSL/rustls host
boundary is implemented. Stage 556 registers `http2` with explicit unsupported
errors and stable session constants; real HTTP/2 remains a future host
networking slice. Stage 557 gives `node:test/reporters` an explicit
unknown-built-in error, documenting the unsupported reporter integration instead
of exposing a generic loader failure. Stage 558 gives `node:sqlite` an explicit
unknown-built-in error until a SQLite host/runtime dependency is intentionally
added. The full focused-suite audit after stage 558 found six failures caused by
override ordering: OS helpers replaced existing environment-aware methods,
`fs.constants` omitted access flags, and `sys` returned a fresh util clone. The
follow-up preserves existing OS behavior, adds `F_OK`/access flags, and caches a
shared util object; all six stages now pass before the next slice.
