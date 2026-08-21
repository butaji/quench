# Global surface — every Node `globalThis` name

> Contract: This task is part of broad Node 24 compatibility across Linux
> x86_64, Linux ARM64, macOS, and Windows. Native addons and Node-API are
> excluded. Use the statuses and release gates in
> [compatibility-contract.md](../docs/compatibility-contract.md).

## Contract alignment

This task supports the Node 24 application-runtime contract on Linux x86_64;
observable Node behavior remains the compatibility target.
See `docs/authoritative-test-sources.md` for the Node, LLRT, Deno, WPT, and
Test262 reference roles.

## Goal

Make global names and installation rules declaration data consumed by one
generated installer and generated surface checks.

Every name that Node exposes on `globalThis` either exists in
`crates/quench-node/polyfills/bootstrap.js` with Node-accurate behaviour or
returns the Node-correct "not supported" error. The grep of
`globalThis.<name> =` in the polyfill matches the canonical list below.

## Canonical list

The set of names Node attaches to `globalThis`, taken from the
`tests/node/test/parallel/*` fixtures that read them. Grouped by origin.

### Always available (polyfill responsibility)

- `globalThis` — built-in.
- `global` — alias to `globalThis`.
- `process` — object (task 008 / task 011).
- `Buffer`, `buffers` — `Buffer` polyfill (task 001).
- `console` — methods (task 011).
- `queueMicrotask` — host or polyfill (already present).
- `structuredClone` — polyfill (task 011).
- `setTimeout`, `setInterval`, `setImmediate`, `clearTimeout`,
  `clearInterval`, `clearImmediate` — timers (task 003).
- `setImmediate` — done.
- `atob`, `btoa` — done.

### URL

- `URL`, `URLSearchParams` — done (task 007).

### Encoding

- `TextEncoder`, `TextDecoder` — done.

### Web Crypto (Browser-style)

- `crypto` — `getRandomValues`, `randomUUID`, `subtle`. Surface lives in
  task 011 (`crypto` module); the global alias is just a re-export.

### Streams (web)

- `ReadableStream`, `WritableStream`, `TransformStream`,
  `ReadableStreamDefaultController`,
  `WritableStreamDefaultController`, `TransformStreamDefaultController`,
  `ByteLengthQueuingStrategy`, `CountQueuingStrategy`.
  Backed by `stream/web` polyfill (task 011).

### Channels / messaging

- `BroadcastChannel` — class.
- `MessageChannel`, `MessagePort` — class pair; in-process delivery via a
  shared `EventTarget`.
- `CustomEvent` — class.
- `Event` — class.
- `EventTarget` — class.
- `AbortController`, `AbortSignal` — done (task 005 / task 008 follow-up).
- `DOMException` — class with `name`, `message`, `code` and the standard
  `code` table.

### Performance

- `performance` — `now`, `timeOrigin` (task 003).

### Errors

- `Error`, `TypeError`, `RangeError`, `SyntaxError`, `ReferenceError`,
  `EvalError`, `URIError` — built-in.
- `AggregateError` — built-in.
- `DOMException` — see Channels.

### Misc

- `navigator` — `{ userAgent, language, languages, hardwareConcurrency,
  platform }`. Pure JS object.
- `fetch` — global fetch. Real implementation via host TCP+TLS or
  polyfill via the `http` polyfill (task 011 follow-up).
- `FormData`, `Headers`, `Request`, `Response` — WHATWG. Polyfill or
  re-export from `node:undici` (not in this scope; track as
  `fetch`-bundled).
- `Worker` — web worker. Out of scope on this target (return
  `Error("Worker is not supported")`).
- `SharedArrayBuffer`, `Atomics` — built-in (rquickjs feature flag).
- `FinalizationRegistry`, `WeakRef` — built-in.
- `gc` — Node extension. In the harness, expose a no-op or a real
  hook (host decision).
- `require`, `module`, `exports`, `__filename`, `__dirname`,
  `__quench_*` — task 006 / task 011.
- `process` — task 011.

### V8

- `v8` — module (task 011).

## Slicing rules

- One stage per `Stage N: <global>` row.
- Each stage must run the global under both `node` and `quench-node` and
  the diff in observable behaviour must be empty (or only the documented
  omissions).
- Stream/web globals are gated on `stream/web` polyfill (task 011).

## Done when

- `grep -E '^globalThis\.[A-Za-z_$0-9]+ =' crates/quench-node/polyfills/bootstrap.js`
  matches the names above (modulo intentional omissions).
- Every channel/EventTarget/CustomEvent stage passes.
- `navigator` and `performance` stages pass.
- DOMException stage passes (covers the `code` table).

## Status

Complete for the supported Linux/macOS runtime surface via tasks 001, 003,
005, 007, 008, and 011. EventTarget,
CustomEvent, MessageChannel/Port, BroadcastChannel, and streams/web are
covered by the existing focused stages. The navigator surface is covered by
stage 2555, and the DOMException code table and string behavior are covered by
stage 2556. The global fetch/request/response/body surface is covered by
stages 2044, 2230–2232, and 2234. The remaining cross-platform global
descriptor audit is covered by stage 1164; platform-specific omissions remain
classified by the compatibility contract.
