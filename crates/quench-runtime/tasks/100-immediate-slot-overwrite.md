# 100 — Ownership-aware immediate slot overwrite

Status: complete

Native samples in [[43-aot-codegen-quality-audit]] show `Value` drop glue consuming
24.4% of Crypto and a large fraction of Navier-Stokes top-of-stack samples. A normal
Rust assignment to `Value` always invokes `Drop` on the old word; `Drop` then tests its
tag and does nothing for numbers, booleans, null, and undefined. Numeric bytecode thus
pays an out-of-line ownership dispatch on nearly every destination write.

Add one canonical `Value::overwrite` operation. If the old slot contains a heap tag,
use ordinary assignment so Rc ownership is released exactly once. If it contains an
immediate tag, use `ptr::write` because there is no resource to release. Route dynamic
register writes, fixed-local writes, and dense-array element replacement through this
operation. This is the `Immediate + HeapOwned` coproduct eliminator at the storage
boundary; callers do not reproduce tag logic.

Acceptance: add an Rc-count ownership test, pass all semantic/stencil tests and full
smoke, and clear an exact alternating full-suite A/B against the accepted task-97
binary. Record both native-profile motivation and benchmark result. Revert on an
aggregate regression or component-floor failure.

## Result: accepted

`Value::overwrite` is the single ownership eliminator. Dynamic register writes, fixed
local writes, and replacement of existing dense-array elements use it; heap-tagged old
values still take ordinary Rust assignment and release Rc ownership exactly once, while
immediate old words use `ptr::write`. The dedicated Rc-count test covers immediate to
heap, heap to immediate, and final heap destruction. All 42 release tests pass, the
executable remains 2,990,672 bytes, and the full smoke record is
`reports/immediate-overwrite/smoke.jsonl`.

The initial three-repetition A/B in `reports/immediate-overwrite-ab/comparison.txt`
showed +7.67% aggregate but contained unstable Earley-Boyer outliers and therefore was
not accepted. The confirming alternating six-repetition run in
`reports/immediate-overwrite-ab-6/comparison.txt` passes every component:

- aggregate: 703.400 → 773.574 (+9.98%)
- Richards: +5.32%; DeltaBlue: +2.82%; Crypto: +34.78%; RayTrace: +5.91%
- Earley-Boyer: +4.82%; RegExp: +0.87%; Splay: +4.08%; Navier-Stokes: +25.79%

This confirms the native sample attribution: redundant ownership dispatch on immediate
slot overwrites was a material cross-suite cost.
