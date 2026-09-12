# 393 — Composable native-path instrumentation stencils

Status: in_progress

Instrument the same native stencil graph used by release execution. The current diagnostic
mode disables direct blocks so instruction/opcode counters cannot be skipped; that changes
the program being observed and prevents reliable native-cover attribution.

Model an observation as an endomorphism over a physical connector context:

```text
Observe<Event, Gamma>: Gamma -> Gamma
```

It preserves every live register, frame slot, PC state, ownership fact, and continuation.
Its only added effect is an explicit diagnostic writer token. Instrumentation is a pure
quote rewrite that inserts observation nodes before final layout. The ordinary release
functor maps every observation node to `id_Gamma`; the diagnostic functor maps it to an AOT
cooked counter stencil. Both then use the same costed cover, two-pass linker, and published
function graph.

## Canonical data and linking

Define one `ObservationPlan { event, site, payload, connector }` sum. Derive counter layout,
stencil selection, DWARF/JIT symbol labels, and JSON reports from it. Do not add mode tests
inside ordinary opcode stencils and do not mutate shared kernels.

Build separate immutable diagnostic images. Counter addresses and event/site identifiers are
ordinary Task 390 patch bindings. A counter stencil performs the smallest target-supported
operation and tail-composes with the original successor. Where a helper is unavoidable, use
an explicitly costed shared diagnostic kernel; the release image must contain neither the
edge nor a dormant branch.

Initial events:

- stencil template and shared-kernel entry;
- native-to-kernel and kernel-to-native seam;
- typed guard success and side exit;
- IC recipe arm, miss, and invalidation;
- guest call entry, native return continuation, and Rust fallback;
- feedback-field write/read for Task 392.

## Acceptance

- Enabling diagnostics does not disable or replace direct blocks, numeric regions, property
  stencils, call continuations, or adjacent fallthrough.
- For a fixed test, block/opcode/event totals reconcile with control-flow invariants and the
  previous semantic profiler where both are valid.
- Release and identity-instrumented images are byte-identical; release disassembly contains
  no diagnostic branch, counter address, or helper symbol.
- Diagnostic images preserve connector/category laws and pass the complete correctness suite.
- Native samples resolve generated functions and inserted observation stencils through a
  deterministic symbol/source map.
- Measure instrumentation overhead by event family. Overhead is reported, not hidden, and
  no observed profile is used as a runtime hotness selector.

Primary source: copy-and-patch instrumentation can be implemented by injecting stencils at
compile/link time, while standard compiler output supplies native debug information:
<https://doi.org/10.1145/3828170.3828176>.

This task supersedes only Task 93's need to disable direct blocks in instrumented images.
Task 93's zero-overhead release split remains valid; Tasks 81, 166, 331, 377, and 382 consume
the new honest event stream.

## First implemented slice

`deegen_observe_entry` is one rustc/LLVM-cooked `Connector -> Connector` template. Its
counter address is a four-lane raw-value patch demand, and its terminal branch is the
ordinary internal-next hole. AArch64 disassembly is seven instructions: materialize the
counter address, relaxed `LDADD`, and tail branch. There is no helper call or extra frame
field.

The quote stage conditionally composes this same leaf before labeled direct-block,
direct-opcode, numeric-region, semantic-kernel, effect-reentry, and direct-call entries.
The label belongs to the composite so incoming branches cannot bypass observation. With
the mode disabled, the combinator returns the original labeled leaf and therefore emits
no observation bytes or branch. `DEEGEN_NATIVE_PATH_STATS` selects the diagnostic functor;
it does not set the legacy `instrumented_kernels` flag and does not disable direct
selection.

The first 20 ms V8v7 census is in `reports/task393-native-path-census/`. It proves the
previous profiler hid most executed native code. Selected examples:

| Suite | Direct blocks | Direct opcodes | Numeric regions | Semantic kernels |
|---|---:|---:|---:|---:|
| Richards | 556,512 | 2,892,748 | 23,233 | 694,529 |
| Crypto | 374,575 | 1,153,857 | 388,524 | 349,494 |
| RayTrace | 35,779 | 3,024,968 | 1 | 322,259 |
| EarleyBoyer | 4,437,341 | 5,332,131 | 14,093 | 1,465,914 |
| NavierStokes | 25,524 | 21,191 | 24,554 | 260 |

A Richards run with both native-path and ordinary structured link statistics reported
107 compiled direct blocks and 437 compiled direct opcodes while the execution counters
were active. Thus instrumentation observes, rather than replaces, the direct graph.

Remaining work: add site/recipe identifiers and guard/IC/call-edge events; reconcile
block-level totals against control-flow invariants; generate the native symbol/source map;
and measure observation overhead by family. Exact V8v7 acceptance is not required for a
diagnostic-only image, but disabled-mode byte identity and full correctness remain hard
requirements.

## Guard-failure attribution required by Task 396

The next event family records `region_start`, `GuardSource`, `GuardKind`, and the exact
`GuardFailure` constructor. Property failures additionally record the property-site PC and
`PropertyGuardFailure` constructor, but not the property spelling. Success counts are keyed
by the same normalized region identity so pass/fail totals reconcile.

Aggregate in a separately linked diagnostic image. The release functor still erases every
observation to identity, and enabling this event family must not replace a direct region with
the semantic executor. Report only a named bounded number of sites using
`MAX_REPORTED_GUARD_FAILURE_SITES`; retain complete totals by failure kind when the site
report truncates. These events diagnose which prelinked native alternatives Task 396 needs;
they may never choose code by execution heat.

This guard-attribution slice is now implemented. `NUMERIC_REGION_STATS_JSON` reports
complete totals by constructor, a bounded deterministic site table, and success totals by
region. Site identity includes source id, region PC range, source byte offset, normalized
source kind/index, requirement kind, property PC where applicable, and failure byte offset.
Captured names and property spellings are intentionally absent. Collection is gated by the
existing diagnostic-image flag and does not influence linking or runtime selection.

The first census is stored under `reports/task396-guard-failure-census/`. It identified the
general property-result representation bug and the downstream reaching-definition bug now
being repaired in Task 396. Remaining Task 393 work is IC/call-edge observation,
control-flow reconciliation, native symbol/source-map generation, and disabled-mode binary
identity verification.
