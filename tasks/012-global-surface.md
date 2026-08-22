# Global surface — every Node `globalThis` name

> Contract: This task is part of broad Node 24 compatibility across Linux
> x86_64, Linux ARM64, macOS, and Windows. Native addons and Node-API are
> excluded. Use the statuses and release gates in
> [compatibility-contract.md](../docs/compatibility-contract.md).

## Bun matrix and verification policy

The Bun reference matrix is at
`https://bun.com/docs/runtime/nodejs-compat` (currently describing Node 26);
this project targets Node 24. Bun green/yellow/red labels are reference
classification, not Quench evidence. A global is green only after focused and
applicable upstream Node API tests pass; partial or platform-limited behavior
is yellow; unsupported behavior is red. Installation and shape checks alone do
not complete this task. Record exact fixtures, commands, failures, and panics.

## Contract alignment

Every Node global MUST have an explicit green/yellow/red classification.
Green requires applicable Node API tests to pass; yellow records partial or
platform-limited behavior; red records unsupported behavior and the
Node-compatible error or absence contract. Installation, constructor shape, or
an unresolved "real or no-op" alternative is not green evidence.

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
- `SharedArrayBuffer`, `Atomics` — built-in (engine feature flag).
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
`tests/node-compat/stage-2555/navigator-global.js`, and the DOMException code
table and string behavior are covered by
`tests/node-compat/stage-2556/domexception-code-table.js`. The global
fetch/request/response/body surface is covered by stages 2044, 2230–2232, and
2234. The remaining cross-platform global descriptor audit is covered by
stage 1164; platform-specific omissions remain classified by the
compatibility contract.

## Definition-of-done evidence

- Focused gate: `cargo run -p quench-node-test --bin run-compat -- --quiet`
  — **80 passed, 0 failed, 80 total** (2026-08-21).
- Upstream Node fixture gate: `cargo run -p quench-node-test --bin run-parallel`
  — **276 passed, 0 failed, 276 total** (2026-08-21).
- Remaining gap: platform-specific omissions remain classified by the
  compatibility contract; aggregate gates do not provide per-global
  differential attribution.
