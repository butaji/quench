# Source locations within fused field operations

Frozen source cf85930513380205d8b9ea01ef1f162b3e3f4676 passes 363/363 serial comparisons against stable Node v26.12.0 (exit zero, 229.38 seconds), 654 runtime tests, workspace/all-target checking, 43 framework fixtures and nine no-JIT web correctness targets. CLI, runner and test hashes remain fixed during the full API run. The base source, patch and added fixture hash reconcile to this commit. Later eval-origin work is excluded.

The retained negative control shows this.a.b reports b's source position for both getters. Field sites now retain each read/read/write coordinate. Scoped active-component state selects the executing operation and restores the previous context after success or failure. General and specialized execution, decoded binaries, setters, throwing getters and subsequent captures are checked. Bytecode format 91 serializes the coordinates; validation rejects zero coordinates and the reader rejects invalid option tags. The sink probe reproduced the first-read error; its setter position already matched before this repair.

The first complete runtime build fails with ENOSPC before tests. Its log is retained separately from the successful 654-test retry. Completed compiler caches were removed; binaries, SDKs and evidence were preserved.

All 32 fresh matched instruction captures are valid with unchanged frozen binary identities. Measurements compare against frozen 03ea582d2, use startup subtraction and exact completed fixture counts, and cover all eight fixtures. They are integrated comparisons, not isolated attribution, throughput, RSS or a benchmark win.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
| crypto | +0.5642% | pass |
| deltablue | -34.2940% | pass |
| earley-boyer | +1.6031% | FAIL |
| navier-stokes | -0.9796% | pass |
| raytrace | -11.9784% | pass |
| regexp | -6.1313% | pass |
| richards | -31.2661% | pass |
| splay | +0.9304% | pass |

Startup increases by 37,115,687–246,419,562 instructions. Total Node parity and web benchmark dominance remain unproven. Eval/sourceURL/source maps, the complete CallSite surface, internal string-key visibility and broader Node APIs remain open.
