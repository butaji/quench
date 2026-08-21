# Global API coverage

Target Bun green/yellow globals: AbortController/Signal, Blob, fetch, FormData, Headers, MessageChannel/Port, compression streams, Readable/Writable/Transform streams and strategies, TextDecoder/Encoder and streams, URL/URLSearchParams, structuredClone, SubtleCrypto/Crypto/CryptoKey, WebAssembly, DOMException, Event/EventTarget/CustomEvent, BroadcastChannel, timers, performance/PerformanceObserver/PerformanceResourceTiming, Request/Response, global/module/exports.

Each global needs an actual constructor/function surface, documented partial gaps, focused fixture, lint-compliant implementation, and current verification evidence.