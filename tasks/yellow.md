# Yellow Node API coverage

Target Bun-documented partial behavior for diagnostics_channel, https, async_hooks, child_process, cluster, crypto, domain, module, perf_hooks, process, util, tls, v8, vm, wasi, worker_threads, inspector, repl, node:test.

Document every supported API and every intentional Bun-matching gap. Implement reachable behavior with real errors/backends; no shape-only claims. Enforce file <=500 lines, function <=40 lines, complexity <=10. Add focused fixtures and run applicable upstream tests.
## Status (2026-08-21; measured after latest merge)

The listed yellow modules have partial implementations and focused fixtures.
The current focused suite passes 59/59 and the upstream parallel manifest
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
59/59, upstream parallel 178/178. Built-in diagnostics channels
(`http.client.*`, `http2`, `dgram`, `http.server.*`, `net`, `module`,
`console`, `child_process`, `worker_threads`) and Node's channel store semantics
remain an explicit gap, not a compatibility claim.
