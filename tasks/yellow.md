# Yellow Node API coverage

Target Bun-documented partial behavior for diagnostics_channel, https, async_hooks, child_process, cluster, crypto, domain, module, perf_hooks, process, util, tls, v8, vm, wasi, worker_threads, inspector, repl, node:test.

Document every supported API and every intentional Bun-matching gap. Implement reachable behavior with real errors/backends; no shape-only claims. Enforce file <=500 lines, function <=40 lines, complexity <=10. Add focused fixtures and run applicable upstream tests.
## Status (2026-08-21; measured after latest merge)

The listed yellow modules have partial implementations and focused fixtures.
The current focused suite passes 68/68 and the upstream parallel manifest
passes 178/178. This verifies only the repository's current Node API manifests;
it does not erase Bun-documented partial behavior or prove full Node v26
compatibility. Each module MUST record supported APIs, intentional gaps,
focused results, and applicable upstream Node API results.

Known intentional or measured partial surfaces include `tls`, `https`,
`async_hooks`, `child_process`, `cluster`, `crypto`, `domain`, `module`,
`perf_hooks`, `util`, `v8`, `vm`, `wasi`, `worker_threads`,
`inspector`, `repl`, and `node:test`. Each module MUST record supported APIs,
intentional gaps, focused results, and applicable upstream Node API results.

Measured (2026-08-21): `node:process` now provides `process.uptime()` (seconds
since host start via `std::time::Instant::now()`) and `process.memoryUsage()`
returning a sysinfo-backed RSS estimate with `rss`/`heapTotal`/`heapUsed`/
`external`/`arrayBuffers` keys. Verified by
`crates/quench-node-test/node-tests/test-process-uptime-memusage.js`; focused
suite 70/70, upstream parallel 178/178. Remaining `process` gaps include
`fork` (IPC), `exec`/`execFile` async child handles beyond the synchronous
forms, and `process.resourceUsage()` exact POSIX values.
“Implemented” means surface code exists; it MUST NOT be reported as verified
without executable test evidence.

Measured additions (2026-08-21): `diagnostics_channel` now provides
`channel()`/`subscribe`/`unsubscribe`/`channelNames`, `tracingChannel` with a
real `traceSync`/`tracePromise` lifecycle (start/end/asyncStart/asyncEnd/error
dispatch and error propagation), and `boundedChannel`. Verified by
`crates/quench-node-test/node-tests/test-diagnostics-channel.js`; focused suite
68/68, upstream parallel 178/178. Built-in diagnostics channels
(`http.client.*`, `http2`, `dgram`, `http.server.*`, `net`, `module`,
`console`, `child_process`, `worker_threads`) and Node's channel store semantics
remain an explicit gap, not a compatibility claim.

Measured (2026-08-21): `node:crypto` provides pure-Rust digests, `getHashes`,
`getRandomValues`, and `crypto.subtle` with working `digest` (SHA-1/256/384/512),
`importKey`, `generateKey` (AES-GCM 128/256 + HMAC SHA-256, CSPRNG-backed),
`encrypt`/`decrypt` (AES-256-GCM via the `aes-gcm` crate, round-trip +
wrong-IV failure verified), `exportKey` (raw format), `deriveBits`
(PBKDF2-HMAC-SHA256 verified against RFC 6070 vector 5 byte-for-byte),
`deriveKey` (wraps PBKDF2 output as an AES-GCM/HMAC secret CryptoKey), and
`sign`/`verify` (HMAC-SHA-256 and HMAC-SHA-1, verified against RFC 4231 test
case 1 byte-for-byte). Verified by `test-crypto-subtle-aes-gcm.js`,
`test-crypto-subtle-generate-key.js`, `test-crypto-subtle-pbkdf2.js`, and
`test-crypto-subtle-hmac.js` (focused 68/68, upstream parallel 178/178).
Algorithm breadth (no `encapsulate`/`decapsulate`, `ed448`, `x448`, `rsa-pss`,
`dsa`, `dh`, extra curves, or CCM/OCB/XTS/`chacha20-poly1305`) is similarly
out of reach without further crypto backends. These are tracked here, not
claimed.
