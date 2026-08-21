# Yellow Node API coverage

Target Bun-documented partial behavior for diagnostics_channel, https, async_hooks, child_process, cluster, crypto, domain, module, perf_hooks, process, util, tls, v8, vm, wasi, worker_threads, inspector, repl, node:test.

Document every supported API and every intentional Bun-matching gap. Implement reachable behavior with real errors/backends; no shape-only claims. Enforce file <=500 lines, function <=40 lines, complexity <=10. Add focused fixtures and run applicable upstream tests.
## Status (2026-08-21; measured after latest merge)

The listed yellow modules have partial implementations and focused fixtures,
but they are not fully Node-compatible. `run-compat --quiet` currently passes
49/57 fixtures overall and fails `events`, HTTP, `net`, and `readline` cases.
`run-parallel` is currently blocked by an uncaught panic in
`crates/quench-runtime/src/intl/datetime_format_date.rs:90`; no upstream
green/yellow pass rate is claimable.

Known intentional or measured partial surfaces include `tls`, `https`,
`async_hooks`, `child_process`, `cluster`, `crypto`, `domain`, `module`,
`perf_hooks`, `process`, `util`, `v8`, `vm`, `wasi`, `worker_threads`,
`inspector`, `repl`, and `node:test`. Each module MUST record supported APIs,
intentional gaps, focused results, and applicable upstream Node API results.
“Implemented” means surface code exists; it MUST NOT be reported as verified
without executable test evidence.
