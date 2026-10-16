# Uncaught monitor ordering and shared process event dispatch

Frozen source b6dfc399691e600e719fff167c0a87abf2ef6f38 passes 396 serial Node v26.12.0 API comparisons (225.21s), workspace/all-target, five focused exception comparisons, 43 framework fixtures and nine no-JIT web correctness targets. Before/after manifests are identical. Runtime and RegExp source plus Core Cargo inputs match the 671-test passing f4e6ed305 tree; later source is excluded.

Uncaught delivery emits uncaughtExceptionMonitor before selecting the primary capture slot. The monitor may replace that slot. Delivery reads captureFn again for its actual property call, matching the original SDK accessor order. A shared process-event call owner replaces duplicated dispatch; monitor results are ignored, whereas uncaught results use standard truthiness. Permanent oracle cases cover primary/replacement/fallback transitions, ignored Symbol returns, truthy string results and a getter selecting different callbacks for its first and second reads. Initial failing outputs are retained.

All 32 fixed-work captures are valid with unchanged identities against frozen 03ea582d2. Candidate is iteration opt1/256 CGUs/no LTO/incremental=false, with integrated regex/link-order changes. Earlier baseline builds used different configuration; these frozen-executable deltas do not isolate source attribution or qualify production throughput/RSS. The verifier may record a later working-tree revision while profiling; the frozen CLI hash and source manifest establish candidate provenance.

| Fixture | Instruction delta | 1% guard |
| --- | ---: | --- |
| crypto | -3.3147% | pass |
| deltablue | -37.9457% | pass |
| earley-boyer | -4.0591% | pass |
| navier-stokes | -6.5108% | pass |
| raytrace | -15.4410% | pass |
| regexp | -6.8464% | pass |
| richards | -39.2019% | pass |
| splay | -9.4239% | pass |

Auxiliary capture, report flags, abort toggles, async hooks and exact fatal lifecycle/source fidelity remain open. Total Node parity and a benchmark win are not established; no authored performance optimization accompanies these correctness fixes.
