# Function public names, debug origins and native-info lookup

Frozen source 3e349f1775f1b15feae31b6d992687441ffdd911 passes all 376 serial stable Node v26.12.0 API comparisons (264.88 seconds), 659 runtime tests, workspace/all-target checking, two added focused comparisons, 43 framework fixtures and nine no-JIT web correctness targets. Source, oracle, CLI, runner and test identities remain unchanged through the full API run. Manifest file hashes reconcile to the source commit; the retained frozen CLI remains unchanged. Later host builds and uncommitted Function-constructor source-layout changes are excluded.

Dynamic constructors initialize anonymous as public creation data without marking a user name reconfiguration. Debug names derive from actual program origin; stack/detailed names and getter-free constructor display distinguish public anonymous from the empty source name across four constructor kinds. Descriptor mutations, throwing getters, retained CallSites and prototype roles are covered.

Prototype setup and named native-info lookup are explicit lifecycle states. Function name/length descriptor transitions share the immutable shape representation; compaction preserves them. Native accessor visibility derives from ordered descriptors, including deletion/restoration of length. The 64-row matrix matches both ordinary and jitless stable Node; the core regression passes in both execution modes. An initial wider 48-row probe exposes three deleted-length differences, then the descriptor-position repair resolves them. The first workspace check rejects duplicated match arms; corrected checks pass. A file-mode supplemental invocation is rejected by both runtimes because the repository's js files are ESM; the intended eval-mode comparison succeeds with empty stderr and exit zero.

A separate 80-row integrity/warm-up control still has three differing rows (readBefore/missing, own and computed). The parent frozen 74c has 21 differences on that same input, and no previously matching row becomes different. These are row comparisons, not distinct bug counts or full conformance gains. V8 lazy feedback allocation and prototype-fast lookup remain incompletely modeled; no guessed invocation threshold or fixture policy is added. An isolated GC probe does not support attributing the original difference to weak ownership.

All 32 matched fixed-work instruction captures are valid with unchanged frozen identities against 03ea582d2. Only two of eight 1% guards pass. These integrated deltas include shared changes since the baseline and do not isolate attribution to this repair. The new named lookup hook's placement requires investigation; no authored performance optimization or throughput/RSS claim is made. A subsequent source-layout build runs alongside part of instruction collection; wall times are not compared, and the measured executables are frozen separately.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
| crypto | +14.8569% | FAIL |
| deltablue | +19.3109% | FAIL |
| earley-boyer | +24.1446% | FAIL |
| navier-stokes | -1.4290% | pass |
| raytrace | +33.3824% | FAIL |
| regexp | +0.1000% | pass |
| richards | +33.3318% | FAIL |
| splay | +36.4358% | FAIL |

Startup increases by 801,435,723–1,481,442,753 instructions. Guard failure is retained, not reported as a pass or benchmark win. The 20-case body-layout negative control, confirmed ordinary NUL-key visibility failure, warm-up transitions, raw UTF16 source, sourceURL/source maps, other stack methods and the broader Node audit remain open. Total Node parity and a valid winning web throughput result are not established.
