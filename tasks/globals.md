# Global API coverage

Target Bun green/yellow globals: AbortController/Signal, Blob, fetch, FormData, Headers, MessageChannel/Port, compression streams, Readable/Writable/Transform streams and strategies, TextDecoder/Encoder and streams, URL/URLSearchParams, structuredClone, SubtleCrypto/Crypto/CryptoKey, WebAssembly, DOMException, Event/EventTarget/CustomEvent, BroadcastChannel, timers, performance/PerformanceObserver/PerformanceResourceTiming, Request/Response, global/module/exports.

Each global needs an actual constructor/function surface, documented partial gaps, focused fixture, lint-compliant implementation, and current verification evidence.
## Status (2026-08-21)

Implemented and installed in host: `Event`, `CustomEvent` (Event-inherited, preventDefault/stopPropagation), `BroadcastChannel` (named delivery/close), `ReadableStream`/`WritableStream`/`TransformStream` (promise-based reader/writer), `TextDecoderStream`/`TextEncoderStream` (wrap TextDecoder/TextEncoder), `CompressionStream`/`DecompressionStream` (format-validated transform-like surfaces), `crypto` (getRandomValues + subtle.digest/importKey), plus existing Headers/FormData/Blob/MessageChannel/MessagePort/Request/Response/DOMException/URL/URLSearchParams/TextDecoder/TextEncoder/timers/structuredClone/fetch/AbortController/AbortSignal/EventTarget/console/process/Buffer.

Remaining Bun-documented gaps: `WebAssembly` binding absent, `performance`/`PerformanceObserver` globals (module-exposed in perf_hooks only), `module`/`exports`/`require.resolve` globals, FormData/Headers iterators and forEach, Blob stream()/bytes(), MessagePort addEventListener model, crypto.subtle encrypt/decrypt/sign/derive (return undefined). All installed globals have fixture coverage in test-web-globals.js; compression/text stream variants are transform-like (no real codec). Verified: run-compat 57/57, run-parallel 178/178, workspace 76.
