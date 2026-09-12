# 130 — Static-property numeric region stencils

Status: complete

Extend the flat `RegionOp` coproduct with typed static-property numeric reads and writes.
Compose them with the existing local/literal/arithmetic/branch leaves into one guarded
basic-block or traced-loop stencil. Rustc/LLVM AOT handlers index the Task 129 property
views and move raw numeric words; the entry guard establishes all bounds, shapes,
ownership, and numeric tags once, so individual leaves contain no repeated shape or
drop checks.

The category objects distinguish `UnknownFrame`, `ValidatedNumericProperties`, and the
ordinary connector context. Guard failure is a coproduct edge to the canonical slow
block; successful composition remains a single linked memory function. Symbolic CFG
labels and continuation holes are handled by the existing linker. Selection is a pure
capability predicate over opcode/effect shape and liveness, independent of names,
locations, workload identity, or runtime heat.

Acceptance: quote/requirement/escape tests, extracted relocation validation, fast and
slow semantic agreement, direct-entry counters, complete smoke, and a stable full-suite
A/B with no component below the standing floor. Record and revert if guarded coverage
or code size cannot amortize entry validation.

## Work log

The initial implementation extends the canonical `RegionOp` sum with static reads and
writes, derives property demands from receiver dataflow, installs Task 129 guarded slot
views at the region boundary, and selects two rustc/LLVM-cooked tail-composable leaves.
The copy-and-patch linker uses the existing flat free sequence and symbolic loop trace;
there is no runtime code generator, heat counter, or benchmark-specific selector.

Seventy-one release tests pass, including an end-to-end JS loop that verifies its
numeric result and proves that exactly one composed region was linked. The complete
eight-suite smoke passes in `reports/task130-static-property-region-smoke.jsonl`.
Fresh 20 ms counters found 29 linked regions and 1,796,041 native iterations in Crypto;
Earley-Boyer linked eight regions but all 636 entries failed their numeric guards.
Other newly affected suites had no property-region coverage.

The first alternating focused measurement in
`reports/task130-static-property-focused-ab-6/comparison.txt` regressed Crypto 2.54%,
Earley-Boyer 1.28%, and aggregate 1.91%. Inspection found that a read followed by a
write of the same `(receiver source, property key)` created two identical entry guards,
two retained owners, and two views. The in-progress normalization now stores that fact
once and derives both site mappings from it, with write capability dominating read-only
capability. This is structural guard-instance sharing, independent of runtime heat.

## Result: completed as score-neutral coverage infrastructure

The final representation removes the two property-only vectors and the added ABI field
from every call frame. Dense arrays and numeric properties now inhabit one validated
memory-view sequence: dense views occupy the prefix and property site literals are
derived by adding that prefix length. Repeated property demands normalize by
`(GuardSource, key)` and share one guard/view; write capability dominates read-only
capability. The common own-property shape chain is inline, and up to the named
`INLINE_PROPERTY_VIEW_CAPACITY` validated views stay inline as well. Region analysis
proves that the local/captured owner root cannot be overwritten, so the installed raw
view borrows that existing root instead of retaining another owner. No magic offsets,
workload identities, or runtime hotness decisions were introduced.

Final evidence:

- 71 release tests pass. The end-to-end property-loop test checks the JS result and
  proves one composed native region was selected.
- `reports/task130-static-property-final-smoke.jsonl` passes all eight suites.
- `reports/task130-static-property-region-stats.txt` records 29 linked Crypto regions,
  161,727 successful guards, and 1,796,041 native iterations. The Task 129 baseline
  linked 15 regions and executed 1,704,849 iterations under the same short probe.
- `reports/task130-borrowed-property-view-crypto-ab-6/comparison.txt` is effectively
  flat at 1356 → 1355.5 (-0.04%).
- `reports/task130-static-property-full-ab-4/comparison.txt` is effectively flat at
  1154.52 → 1155.28 (+0.07%). Its apparent RayTrace floor miss came from one 583-point
  outlier; the six-run recheck in `reports/task130-raytrace-floor-recheck-ab-6/`
  measures 1225.5 → 1223 (-0.20%).

This work is retained because it closes a real general-purpose stencil-vocabulary gap
and is a dependency of Task 128, but it makes no performance-gain claim. The experiments
with per-property frame vectors and duplicated read/write guards remain recorded in the
focused A/B directories above; those representations were rejected. Final executable:
`/tmp/deegen-task130-static-property-region`, SHA-256
`e3ae2ea85beb0cd59e584931d90fe32cacc127ce73acefe48bb0ed50e4620989`.
