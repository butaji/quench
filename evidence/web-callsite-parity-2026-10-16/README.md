# CallSite prototype, identity, receiver descriptors and strict frames

Frozen source 53ba17cbd2053ee868281b66aa67706b4551a5d4 passes 343/343 serial API comparisons against explicit stable Node v26.12.0 with exit zero, 647 Core tests, workspace/all-target checking, 43 framework fixtures and nine no-JIT web correctness targets. CLI, runner and test executable hashes stayed fixed through the API run. The API run starts after compilation and Callgrind finish. Later factory/literal provenance work is excluded.

CallSites share a realm-owned native prototype. Their constructor throws Error("Not supported") on call/new without touching a Proxy newTarget's prototype. Native method descriptors and receiver errors match Node. The constructor's original identity survives public prototype.constructor mutation. Missing methods are not represented as completed APIs.

getFunction returns traced callable identity with strict-frame visibility shared by getThis. The first comparison failed: proper-tail-call elimination removed an observable strict Node caller. The Node host now explicitly retains those frames in both general and specialized dispatch; Core's default proper-tail-call contract and Wasm behavior remain intact. The failed comparison and subsequent passing control are both retained.

The initial 41e9b9f03 qualification failed: 340/343 API tests and three incomplete guard workloads hit a returning-call boundary panic. 53ba17cbd restores fallback when no specialized activation caller exists. All three affected API cases and Earley-Boyer/RayTrace/Richards native fixed work then pass, followed by this fresh complete qualification. The failed snapshot is preserved separately in web-callsite-initial-regression-2026-10-16; its incomplete counts are never used as instruction deltas.

getMethodName uses original creation names and safe receiver descriptor identity, strips accessor prefixes, searches inherited enumerable aliases and returns null for ambiguous distinct aliases. It observes current receiver properties after capture without invoking name accessors, descriptor getters or Proxy traps. Tests cover strict receivers, non-enumerable candidate matches, numeric keys and lone-surrogate names. Captured receivers/callees follow the existing shared stack root owner and its lifetime tests.

All 32 fresh strict matched Callgrind captures are valid with unchanged identities against frozen 03ea582d2. Instruction guards are separate from production throughput/RSS and do not establish a benchmark win.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
| crypto | +0.0453% | pass |
| deltablue | -25.6575% | pass |
| earley-boyer | -0.2449% | pass |
| navier-stokes | -0.1306% | pass |
| raytrace | -14.0292% | pass |
| regexp | -6.8209% | pass |
| richards | -29.6469% | pass |
| splay | +1.6735% | FAIL |

No total Node parity is claimed. Retained paired counterexamples show factory-created function constructor provenance and util.getCallSites placeholder names/positions remain incorrect. Complete stack/eval/source metadata and broad Node API conformance remain open. All original failed controls remain distinguished from repaired snapshot results.
