# TCP default and option backlog parity

Frozen source 88a5c9d57918d859a8bf6359a6a4237184a5939f passes 383 serial stable Node v26.12.0 API comparisons (260.30s), workspace/all-target checks, 43 framework fixtures and nine no-JIT web correctness targets. Source/oracle/CLI/runner/test hashes remain unchanged across APIs. The runtime crate and workspace manifest/lock match a01c1fb08, whose 668 runtime tests pass in 91.78s; the later native lifecycle/constructor changes are excluded.

Node net.js uses 511 when no truthy backlog is supplied. System-call traces previously show Quench ordinary IPv4/default/HTTP listeners use 128, while only reusePort uses 511. A shared native socket-creation path now uses the Node default for all paths and preserves Unix/libuv address-reuse and supported reusePort behavior. Supplied options.backlog applies the JavaScript truthy default before shared ToNumber/ToInt32 conversion. Actual syscall comparisons match Node for four default cases and twenty net/HTTP option cases: numeric/string zero, seven, NaN, infinity, negative, wrapped integers and booleans. The fixture reports unavailable tracing explicitly. ss reports zero queue capacities for both runtimes on this host; equal-zero output is not backlog proof.

The first option check fails on an unused legacy wrapper; removing it restores the check. Retained later lifecycle probes still differ: new-server address(), missing listen generation, supplied-host pending state, positional callbacks and DNS-deferred conversion errors. A Symbol backlog probe prints accepted rows before Node throws asynchronously; synchronous try/catch assumptions are not valid. These remain separate from the passing syscall matrices and are not claimed fixed here.

A fresh unchanged all-nine one-second diagnostic keeps Express JSON invalid/unscored with 249 request timeouts; Hono/Fastify and Node/Bun targets are valid in that short run. Earlier f17 has Express 129 request timeouts and Hono 64 dial timeouts; snapshots include other shared source changes. No isolated attribution, production throughput/RSS improvement, authored optimization or win is claimed.

All 32 fixed-work captures against frozen 03ea582d2 are valid with identities unchanged; only two of eight 1% guards pass. Candidate build: iteration opt1, 256 codegen units, no LTO, incremental=false. Earlier snapshots used incremental compilation, so the frozen-executable comparison includes build-configuration differences and does not isolate source attribution.

| Fixture | Instruction delta | 1% guard |
| --- | ---: | --- |
| crypto | +10.3858% | FAIL |
| deltablue | +13.8813% | FAIL |
| earley-boyer | +16.6957% | FAIL |
| navier-stokes | -6.5016% | pass |
| raytrace | +27.3748% | FAIL |
| regexp | -3.6653% | pass |
| richards | +24.1086% | FAIL |
| splay | +25.9162% | FAIL |

Ordinary NUL string-key visibility, native-source/stack fidelity, complete listen lifecycle and the wider Node backlog remain open. This checkpoint does not establish total Node parity.
