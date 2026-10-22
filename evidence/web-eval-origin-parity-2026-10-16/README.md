# Eval compilation origin

Frozen source bf7bcf5964cd0fc87fabba8461f1b0e1c42c909c passes 364/364 serial stable Node v26.12.0 API comparisons (exit zero, 228.84 seconds), 654 runtime tests, workspace/all-target checking, 43 framework fixtures and nine no-JIT web correctness targets. CLI, runner and test hashes stay fixed through the full API run. The source patch and fixture hashes reconcile before committing identical code. Later parsed-eval changes are excluded.

The retained negative control shows direct eval, indirect eval and Function-constructor frames report isEval=false where Node reports true. Stored compilation origin is independent of binding scope. Captured CallSites copy that classification. Expanded comparisons cover escaped and strict closures, all four Function-constructor kinds, ordinary functions called from eval, host scripts with eval-like filenames and retained CallSites after garbage collection. The repaired negative control matches Node.

All 32 fresh strict matched instruction captures are valid with unchanged identities against frozen 03ea582d2. Startup is subtracted using each binary's own N=0 capture and workloads must finish the exact fixture count. These integrated measurements do not isolate attribution or establish production throughput, RSS or benchmark dominance.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
| crypto | +0.5635% | pass |
| deltablue | -34.2948% | pass |
| earley-boyer | +1.6028% | FAIL |
| navier-stokes | -0.9792% | pass |
| raytrace | -11.9772% | pass |
| regexp | -6.1498% | pass |
| richards | -31.2604% | pass |
| splay | +0.9095% | pass |

Startup increases by 37,204,587–245,298,914 instructions. No authored performance optimization, total Node parity or benchmark win is claimed.

Retained follow-up probes show incomplete CallSite prototype methods, eval-origin descriptions, synthetic filenames and expression-wrapper positions. A valid eval string containing for(;false;) incorrectly throws, and a hashbang Math.max(2,3) script returns undefined instead of 3. Replacing the manual evaluator is subsequent work. SourceURL/source maps, native frame identities, internal-key visibility and broader Node APIs remain open.
