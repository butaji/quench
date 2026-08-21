# Yellow Node API coverage

Target Bun-documented partial behavior for diagnostics_channel, https, async_hooks, child_process, cluster, crypto, domain, module, perf_hooks, process, util, tls, v8, vm, wasi, worker_threads, inspector, repl, node:test.

Document every supported API and every intentional Bun-matching gap. Implement reachable behavior with real errors/backends; no shape-only claims. Enforce file <=500 lines, function <=40 lines, complexity <=10. Add focused fixtures and run applicable upstream tests.
## Status (2026-08-21; measured after latest merge)

The listed yellow modules have partial implementations and focused fixtures.
The current focused suite passes 65/65 and the upstream parallel manifest
passes 178/178. This verifies only the repository's current Node API manifests;
it does not erase Bun-documented partial behavior or prove full Node v26
compatibility. Each module MUST record supported APIs, intentional gaps,
focused results, and applicable upstream Node API results.

Known intentional or measured partial surfaces include `tls`, `https`,
`async_hooks`, `child_process`, `cluster`, `crypto`, `domain`, `module`,
`perf_hooks`, `process`, `util`, `v8`, `vm`, `wasi`, `worker_threads`,
`inspector`, `repl`, and `node:test`. Each module MUST record supported APIs,
intentional gaps, focused results, and applicable upstream Node API results.
“Implemented” means surface code exists; it MUST NOT be reported as verified
without executable test evidence.

Measured additions (2026-08-21): `diagnostics_channel` now provides
`channel()`/`subscribe`/`unsubscribe`/`channelNames`, `tracingChannel` with a
real `traceSync`/`tracePromise` lifecycle (start/end/asyncStart/asyncEnd/error
dispatch and error propagation), and `boundedChannel`. Verified by
`crates/quench-node-test/node-tests/test-diagnostics-channel.js`; focused suite
65/65, upstream parallel 178/178. Built-in diagnostics channels
(`http.client.*`, `http2`, `dgram`, `http.server.*`, `net`, `module`,
`console`, `child_process`, `worker_threads`) and Node's channel store semantics
remain an explicit gap, not a compatibility claim.

Measured (2026-08-21): `node:crypto` provides pure-Rust digests, `getHashes`,
`getRandomValues`, and `crypto.subtle` with working `digest` (SHA-1/256/384/512),
`importKey`, `deriveBits` (PBKDF2-HMAC-SHA256 verified against the RFC 6070
vector 5 byte-for-byte), `deriveKey` (wraps PBKDF2 output as an AES-GCM/HMAC
secret CryptoKey), and `sign`/`verify` (HMAC-SHA-256 and HMAC-SHA-1, verified
against RFC 4231 test case 1 byte-for-byte). Verified by
`test-crypto-subtle-pbkdf2.js` and `test-crypto-subtle-hmac.js` (focused
65/65, upstream parallel 178/178). Measured gap versus Bun: `subtle.encrypt`,
`subtle.decrypt`, `subtle.generateKey`, and `subtle.exportKey` return
`undefined` (no AES-GCM or symmetric-cipher backend installed). Closing them
requires adding an AES-GCM dependency to `crates/quench-node/Cargo.toml` (no
AES crate is currently available in the lockfile). Algorithm breadth (no
`encapsulate`/`decapsulate`, `ed448`, `x448`, `rsa-pss`, `dsa`, `dh`, extra
curves, or CCM/OCB/XTS/`chacha20-poly1305`) is similarly out of reach without
further crypto backends. These are tracked here, not claimed.
