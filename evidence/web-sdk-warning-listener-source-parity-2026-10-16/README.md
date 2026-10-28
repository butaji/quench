# Actual SDK warning callback without internal exposure

Source 55a257c6ee2ee65c5cabd66c2687fcde1f49ab21 passes 411 serial Node v26.12.0 API comparisons (246.35s), 672 Runtime tests (72.50s), workspace/all-target, 43 framework fixtures and nine no-JIT web correctness targets. Before/after API source, CLI, runner, tests and oracle manifests match. All shared GC/array/link-order changes through f20372952 are included. Later source is excluded.

The incoming shared warning optimization installed a synthetic onWarning wrapper when internal exposure was disabled. The new oracle reproduced its observable source mismatch: Node's actual SDK callback has 1546 characters, Quench's wrapper 55; superficially matching name/length/constructor metadata did not establish parity. Failed API output, decoded summary, frozen regression CLI hash and controlled environment outputs are retained. The corrected default listener is always the actual SDK callback; its original console is initialized. A redundant second callback cache and native wrapper loader are removed. Legitimate lazy loading of the actual public emitWarning function remains. Ten focused warning tests pass, including an explicit child with NODE_OPTIONS and NODE_NO_WARNINGS cleared, so internal exposure cannot mask the regression.

All 32 instruction captures are valid with unchanged identities against frozen 03ea582d2; all eight guards pass. Frozen hashes/manifests identify the measured code even if a later revision is recorded by the profiler. Iteration opt1/256 CGUs/no LTO/incremental=false, shared source/configuration changes and concurrent Runtime compilation prevent isolated attribution or production timing/RSS claims.

| Fixture | Instruction delta | Guard |
| --- | ---: | --- |
| crypto | -3.5498% | pass |
| deltablue | -37.9909% | pass |
| earley-boyer | -5.6273% | pass |
| navier-stokes | -6.5116% | pass |
| raytrace | -17.5435% | pass |
| regexp | -2.6790% | pass |
| richards | -39.2022% | pass |
| splay | -8.6886% | pass |

Heap accounting, report generation, SDK/global Console identity, abort/async hooks and broader Node contracts remain open. The latest short Express JSON load has 249 timeouts. Total Node parity and a web throughput win remain unproven; no authored performance tuning is claimed.
