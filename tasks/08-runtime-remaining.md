# Stage 08 — Remaining runtime and web APIs

Resolve the matrix's remaining partial/stub surfaces: `vm`, `worker_threads`, `async_hooks`, `diagnostics_channel`, `trace_events`, `node:test`, `node:sqlite`, `inspector`, `repl`, `wasi`, `v8`, `domain`, plus web streams, compression/text streams, fetch objects, structured cloning, messaging, and complete WebCrypto globals. Keep unsupported APIs explicit and scoped; no empty namespace presented as success.

Run corresponding upstream parallel/es-module fixtures, WPT URL/fetch/encoding/streams/WebCrypto/dom-abort suites, and focused stages 2609, 2614, 2241–2243, 2555–2556. Acceptance: every supported export has behavioral coverage, unsupported exports have documented contract status, and worker/resource cleanup is deterministic.
