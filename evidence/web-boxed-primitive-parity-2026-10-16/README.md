# Genuine boxed primitive data

Frozen source 217e80460a7d411f3eed37d4aab7cde44d6457e2 passes 354/354 serial API comparisons against official stable Node v26.12.0 (exit zero, 227.10 seconds), 651 Core tests, workspace/all-target checking, 43 framework fixtures and nine no-JIT web correctness targets. CLI, runner and test hashes remain unchanged through the API run. Source was built from 73bd7e869 plus the recorded patch and extra fixture; their hashes were reconciled before committing the identical source. Subsequent detailed-stack/call-column work is excluded.

The retained initial probe proves that ordinary objects can forge all five wrapper brands by setting internal-looking public string properties. Node rejects them; the old Quench snapshot accepts them and lets primitive methods read forged values. The repair stores one genuine primitive Value in object extras, then derives brands, primitive methods, constructor display, structured cloning and JSON unboxing from it. Public properties cannot supply or replace that value; Proxy targets do not donate their brand. Redundant marker atoms and JSON wrapper-key caches are removed. BigInt valueOf shares one implementation.

The expanded oracle comparison covers forged objects, all wrong wrapper kinds, detached real wrappers, public-key replacement, Proxy trap avoidance, cloning, intrinsic prototype brands and JSON. A weak-reference lifetime test in both execution modes keeps a detached wrapper's symbol alive through collection, then proves release after dropping the wrapper. The common extras visitor owns the payload root.

Before shared integration, 650 Core tests and the focused Node comparison pass. The first integrated runtime build exits 101 before running tests because the 32 GiB workspace runs out of space. Its compiler diagnostics and source patch are retained. Disposable compiler caches were removed; the published-source retry then passes all 651 tests. This environment/build failure is not represented as a runtime-test regression.

This candidate also includes shared native compression, Node stack/error policy and generator-pattern cleanup already merged through 73bd7e869. Matching codec SDK/compiler/archive provenance and stable-source comparisons are retained. Startup and instruction changes are integrated results, not isolated attribution to boxing.

All 32 fresh strict matched Callgrind captures are valid with unchanged identities against frozen 03ea582d2. Six instruction guards pass; Earley-Boyer and Splay remain above the 1% limit.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
| crypto | +0.4616% | pass |
| deltablue | -34.4063% | pass |
| earley-boyer | +1.6209% | FAIL |
| navier-stokes | -1.0408% | pass |
| raytrace | -10.4121% | pass |
| regexp | -5.9345% | pass |
| richards | -31.2163% | pass |
| splay | +2.9097% | FAIL |

Startup increases by 326,284,104–683,528,137 instructions. This is not production throughput/RSS evidence or a benchmark win. No authored optimization or total Node parity is claimed.

An independent retained probe confirms that ordinary user keys beginning with the internal prefix are hidden by reflection and JSON. That separate namespace problem remains open. Native factory/boxing constructor provenance, complete stack/eval/source metadata, broader inference, HTTP lifecycle/HTTP2, workers, shared memory and other Node APIs also remain open. util.getCallSites still has placeholder behavior in this frozen candidate; its later repair is excluded here.
