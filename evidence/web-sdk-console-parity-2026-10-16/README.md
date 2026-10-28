# Original SDK Console qualification

Frozen source `33118dc695a0207e3c99b04abfa9450f6ac33b91` passes 425 serial Node v26.12.0 API comparisons (252.16s), 672 Core tests (47.24s), 43 framework fixtures and nine no-JIT web correctness targets. Source, CLI, runner, test binary and Node manifests match before/after API qualification. Later heap-census source changes are excluded.

The actual configured SDK owns Console constructor and output policy, global namespace and diagnostics. Duplicate Console/timer policy is removed. SDK SafeMap/SafeWeakMap protect diagnostics storage from public prototype mutation. The original SDK test-binding bridge exposes protected primordials and cached raw native bindings while observing current process warning state.

Retained failures: the initial Console run passed420/421 because diagnostics used public WeakMap.get; protected storage fixes that. The next run passed417/421 because the original SDK test bridge received library adapters rather than raw native objects. Raw-provider correction passed118/120 native cases; remaining warning differences came from startup-intrinsics restoration masking current process state. The final correction passes124/124 native cases and425/425 full API cases without changed deadlines.

All32 fixed-work instruction captures are valid and identities unchanged; all8 guards pass against frozen03ea582d2. Candidate uses iteration opt1/256CGUs/noLTO/incremental=false; shared-source and build differences prevent isolated source or production speed/RSS attribution. Later heap-census compilation ran concurrently with instruction capture; wall times are not compared.

| Fixture | Instruction delta | Guard |
| --- | ---: | --- |
| crypto | -3.8006% | pass |
| deltablue | -40.0295% | pass |
| earley-boyer | -7.9997% | pass |
| navier-stokes | -6.5119% | pass |
| raytrace | -22.0720% | pass |
| regexp | -2.6719% | pass |
| richards | -41.5560% | pass |
| splay | -10.7414% | pass |

These subsets do not establish total Node parity. The prior short Express JSON load reported249timeouts; no framework throughput victory or authored optimization is claimed.

## Optimized-build fix forward

The published heap-census foundation initially failed a normal optimized host build because its iterator and occupied-slice helper still had debug/test/profile gates. The retained debug/profile/Core checks did not cover that configuration. The minimal fix removes the three gates in `heap/slots.rs`; an explicit optimized library check on the isolated published source plus this fix passes (11.00s). The larger live-memory API work was stashed and excluded. Initial failures and corrected check logs are retained.
