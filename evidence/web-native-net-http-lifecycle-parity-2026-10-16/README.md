# Native net lifecycle and one authoritative HTTP listening handle

Frozen source 2e7c81d2158c253c0fcb4d155a203cdac7689fd7 passes 387 serial Node v26.12.0 API comparisons, workspace/all-target checks, 43 framework fixtures and nine no-JIT web correctness targets. Source/oracle/CLI/runner/test hashes remain unchanged across APIs. Runtime source matches 409e83ee9, whose 668 tests pass (64.08s), including expanded ordinary native-constructor metadata with traced data. Later changes are excluded.

Rust receives the complete net.Server.listen argument list. Public method/getter/callback constructor shapes match the oracle; creation names are distinct from private dispatch names. Supplied hosts use the existing public DNS module, so overrides and next-tick timing remain observable. Callbacks retain traced data and compare the actual public listen generation. Tests cover wildcard/host pending state, positional callback/backlog, null new-server address, cancellation, repeated close, DNS iteration and early-return cleanup, callback shape/return value, and GC before callback delivery.

Getter failure closes the DNS iterator while preserving its original RangeError and symbol through GC, even when return also throws. Normal iterator-return failure remains observable. The probe exposed a separate occupied exception-capture slot bug: null reset now precedes the occupied check, invalid types precede duplicate-state errors, and shared coded/type errors preserve original scalar descriptions and messages.

The first full 409e run fails six of 386 APIs (380 pass): four HTTP cases and both HTTP-dependent syscall matrices. The new inherited listening getter rejects legacy HTTP writes to a duplicate boolean. That failed qualification is retained separately in web-net-lifecycle-http-initial-regression-2026-10-16. HTTP now derives listening from its actual handle, clears that handle on close and cancels a pending listening notification. An intermediate binary still emits listening after early close; the explicit Node regression catches it and the corrected full run passes. Raw early callback-name, repeated-close, capture-reset and HTTP failures remain separate from successful results.

All 32 matched fixed-work captures are valid with unchanged identities against frozen 03ea582d2. Candidate uses iteration opt1/256 CGUs/no LTO/incremental=false; earlier snapshots used incremental compilation. Integrated frozen-executable deltas include build-configuration differences and do not isolate source attribution or production throughput/RSS.

| Fixture | Instruction delta | 1% guard |
| --- | ---: | --- |
| crypto | +10.3857% | FAIL |
| deltablue | +13.8816% | FAIL |
| earley-boyer | +16.6954% | FAIL |
| navier-stokes | -6.5016% | pass |
| raytrace | +27.3748% | FAIL |
| regexp | -3.6644% | pass |
| richards | +24.1086% | FAIL |
| splay | +25.9184% | FAIL |

Bound-handle error/reentry details, original JavaScript function-source/native-host-frame fidelity, diagnostic report flags, ordinary NUL/internal-property namespaces and wider Node APIs remain open. No total parity, authored optimization or benchmark win is claimed.
