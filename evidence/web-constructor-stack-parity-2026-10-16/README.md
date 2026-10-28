# Function origins and constructor stack metadata

Frozen source df33c1767aa08898b338d4ea42d6a9215b5072fe passes 348/348 serial API comparisons against official stable Node v26.12.0, 649 Core tests, workspace/all-target checking, 43 framework fixtures and nine no-JIT web correctness targets. CLI, runner and API test hashes remain unchanged through the full API run. Separate boxed-primitive follow-up work is excluded.

Function factory constructors derive from immutable original realm intrinsics, function kind and creation realm. Tests cover main/foreign ordinary, arrow, async and generator functions, native and bound functions, mutated public links and safe debug names. Iterator constructor display uses actual hidden origins and physical prototype roles. Completed generator instances retain their creating function until the instance dies; collection tests cover both execution modes.

CallSite constructor flags derive from actual activation entry in general and materialized specialized frames. Pending construction targets belong to the actual callee. Receiver type names share safe Core constructor display and preserve constructor/global null results, primitive receivers and Proxy trap avoidance. Constructor rendering retains the new prefix.

Variable-initializer debug-name inference flows through result-producing sequences, logical/coalescing expressions and conditionals. It does not change the public name. Slotless shape transitions track native name-property reconfiguration and survive compaction. Reading a name or descriptor preserves that state; replacing its value/attributes materializes the public name. Inference outside those contexts remains open. Bytecode format 90 supports the new metadata opcode; checked dense dispatch falls back for later opcodes.

The initial 992e960e2 snapshot failed 1/348 API comparisons: detached arrays in the direct-record arena reported Object. Its full failed run and fixed hash checks are retained in ../web-constructor-stack-initial-regression-2026-10-16. df33c1767 uses the authoritative array projection and passes the affected case before this fresh complete qualification. Earlier focused constructor comparison also caught missing inferred names; the repaired fixture retains that case.

The first framework invocation incorrectly inherited the child-runner environment, and the first web invocation lacked the compiler PATH. Their diagnostics are retained as invocation errors, not runtime regressions. Corrected framework invocation unsets QUENCH_CHILD_RUNNER; web verification uses the build environment and a fresh output directory.

All 32 fresh strict matched Callgrind captures are valid against frozen 03ea582d2, with unchanged candidate/baseline identities. Startup-subtracted instruction work is separate from throughput, RSS and benchmark dominance.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
| crypto | +0.1510% | pass |
| deltablue | -26.6941% | pass |
| earley-boyer | +3.6146% | FAIL |
| navier-stokes | -1.0413% | pass |
| raytrace | -12.5764% | pass |
| regexp | -6.5297% | pass |
| richards | -31.2142% | pass |
| splay | +1.2057% | FAIL |

No total Node parity or web benchmark win is claimed. Native factory/boxing provenance, broader inferred names, complete stack/eval metadata, util.getCallSites, boxed primitive forgery, internal-key visibility and broad Node APIs remain open at this frozen source. The confirmed boxed-primitive repair is subsequent work.
