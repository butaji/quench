# Fatal exception-handler exit state

Frozen source ffc0f8493eae6db6acfcf2c01c16b19826601a6a passes 400 serial stable Node v26.12.0 API comparisons (223.86s), workspace/all-target, 43 framework fixtures and nine no-JIT web correctness targets. The 19-child oracle matrix and three upstream capture/monitor fixtures pass. API source/oracle/CLI/runner/test manifests are unchanged. Runtime/RegExp and Core Cargo inputs match the integrated 672-test passing 2fdb67fcb tree. Later source is excluded.

A handler failure is a distinct exit cause, with its status derived from the SDK's exit-code enum. It retains the thrown diagnostic, suppresses normal exit events and differs from explicit process.exit requests with codes 1, 7 and 23. The generated binding table uses those same typed enum variants instead of repeating numeric definitions. The matrix covers top-level, tick, promise and auxiliary errors, and explicit requests. Initial exit-1/exit-event failures are retained. The initial direct .js oracle invocation uses this repository's ESM package scope and fails require; the corrected .cjs invocation is separate.

All 32 fixed-work instruction captures are valid with unchanged identities; all eight guards pass against frozen 03ea582d2. Iteration opt1/256 CGUs/no LTO/incremental=false, shared-source and configuration differences prevent isolated source attribution or production throughput/RSS claims. The verifier's final working-tree revision may be newer; the frozen CLI hash and source manifest establish provenance.

| Fixture | Instruction delta | 1% guard |
| --- | ---: | --- |
| crypto | -3.3135% | pass |
| deltablue | -37.9500% | pass |
| earley-boyer | -4.3118% | pass |
| navier-stokes | -6.5111% | pass |
| raytrace | -15.5525% | pass |
| regexp | -6.8051% | pass |
| richards | -39.2017% | pass |
| splay | -9.4189% | pass |

The fresh one-second nine-target load diagnostic remains invalid for Quench Express JSON: 249 two-second timeouts. The other eight targets are valid in this short run. A separate finite 500-concurrent POST batch, with a 30-second deadline, completes every request with correct status/JSON and zero errors on Quench and Node. Quench takes 2571ms under concurrent profiling/build activity; this is a diagnostic observation, not a matched production timing comparison or proof of all traffic patterns.

This frozen CLI still prints the old hard-coded v22.0.0 although its bundled compatibility SDK is v26.12.0; the mismatch is retained in benchmark metadata. The later 6d20fba29 version/order correction is excluded. Report generation, abort/async-hook/source contracts, total Node parity and a framework benchmark win remain open. No authored performance tuning is claimed.
