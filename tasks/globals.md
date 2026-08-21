# Global API coverage

Target Bun green/yellow globals: AbortController/Signal, Blob, fetch, FormData, Headers, MessageChannel/Port, compression streams, Readable/Writable/Transform streams and strategies, TextDecoder/Encoder and streams, URL/URLSearchParams, structuredClone, SubtleCrypto/Crypto/CryptoKey, WebAssembly, DOMException, Event/EventTarget/CustomEvent, BroadcastChannel, timers, performance/PerformanceObserver/PerformanceResourceTiming, Request/Response, global/module/exports.

Each global needs an actual constructor/function surface, documented partial gaps,
focused fixture, and current Node API test evidence. Bun's green/yellow label is
classification metadata, not proof that Quench matches Node.

## Status (2026-08-21; measured after latest merge)

The global surface is mixed: many globals are installed, but WebAssembly,
performance-related globals, module/exports/require globals, stream variants,
and crypto algorithm coverage still have documented gaps. Blob, Headers,
FormData, MessagePort, streams, URL, fetch, crypto, and event globals require
the same executable evidence as module APIs; installation alone is not
verification.

Current repository evidence: `run-compat --quiet` passes 59/59 and
`run-parallel` passes 178/178. This verifies the current manifests, not every
Bun-documented Node v26 global. Remaining global caveats and module-specific
gaps require related Node API fixtures before being marked green.
