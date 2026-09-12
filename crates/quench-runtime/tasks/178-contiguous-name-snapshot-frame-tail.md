# 178 — Contiguous name-snapshot frame tail

Status: complete

Extend Task 168's single owned call-frame range from `locals | registers` to
`locals | registers | name snapshots`. `CallFrameLayout` remains the only source of
offsets and counts. The snapshot base is derived from the end of the register range, and
the total slot count includes the sparse PC-indexed snapshot extent required by linked
name-condition stencils.

Remove the separately allocated `name_snapshot_values` vector from `DynFrame`.
`name_snapshots` points into the owned frame tail for AOT reads and snapshot refresh
writes; releasing the one owned range clears locals, registers, and snapshot owners in
one operation. Capturing functions use the same register/snapshot layout with their
external environment-local pointer.

This is a frame-layout morphism, not a new execution path: kernels and stencil instances
continue consuming the existing `name_snapshots` ABI field. Empty snapshot tails are
the identity and allocate no extra slots. Every offset is named or derived; no literal
layout arithmetic may appear in handlers.

Acceptance: layout, captured-name, overwrite, closure, and release-pool tests; complete
release suite and V8v7 smoke; a targeted Richards alternating A/B against the accepted
Task 168 binary. Keep only if the targeted result improves, then run the full alternating
gate and record exact binary identity.

## Result

Accepted as frame infrastructure with a neutral-positive performance result. The one
owned range is now `locals | registers | snapshots`; capturing frames use
`registers | snapshots` while retaining their external local storage. The separate
snapshot vector is gone, and refresh writes through the existing stencil-visible
snapshot pointer into the owned tail. `CallFrameLayout` derives every base and extent.

Evidence:

- All 81 release tests pass, including the extended contiguous-layout test and the
  captured-name/effect-refresh tests.
- Complete smoke: `reports/task178-contiguous-snapshots-smoke.jsonl`.
- Ten-pair, 500 ms Richards comparison:
  `reports/task178-contiguous-snapshots-richards-ab-10/comparison.txt`, 738.5 to 743.5
  (**+0.68%**).
- Full six-pair, 500 ms comparison:
  `reports/task178-contiguous-snapshots-full-ab-6/comparison.txt`, 1744.69 to 1746.58
  (**+0.11%**). Every component clears the standing floor; the weakest is RegExp at
  -2.23% and the strongest is RayTrace at +3.61%.
- Accepted binary: `/tmp/deegen-task178-contiguous-snapshots`, SHA-256
  `27b77bb0584b629c2657dfabfa9245bd80231c9148b04177f66bbb1a6f6cdbf9`.

The very small full-suite change proves that the extra allocation was not a major score
limit. Task 146 remains open for the actual call/return transition.
