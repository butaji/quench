# 99 — Inline the dense-array computed-access connector

Status: complete

The accepted direct dense-array path in [[12-array-fast-path]] is still entered through
out-of-line `Vm::get_computed_prop` and `Vm::set_computed_prop` calls from the singular
semantic executor. Test a narrow rustc/LLVM split: inline the receiver-tag, numeric-index,
array-kind, and bounds guards into the immutable block kernel, while retaining generic
property conversion, prototype behavior, array growth, and all non-array cases as
out-of-line semantic continuations.

This is a general bytecode specialization for every computed access. It is independent
of source identity and benchmark shape. Categorically it is a guarded coproduct:
`DenseNumericAccess + GenericPropertyAccess`, with both branches preserving the same
input/output connector and canonical semantics.

Acceptance: all semantic/stencil tests and full smoke pass; record binary size and an
exact alternating full-suite A/B against the task-97 binary. Reject and revert if the
larger semantic kernel harms the aggregate or breaches any component regression floor.

## Result: rejected and reverted

The candidate passed all 41 release tests and the full smoke suite in
`reports/inline-dense-array/smoke.jsonl`. It reduced the executable from 2,990,672 to
2,990,432 bytes. The exact alternating three-repetition comparison in
`reports/inline-dense-array-ab/comparison.txt` nevertheless regressed the aggregate
from 695.376 to 693.232 (-0.31%). Crypto fell 0.89%, Navier-Stokes 0.83%, and Splay
1.20%; the intended array-heavy beneficiaries did not improve.

Both `#[inline(always)]` annotations were removed. Together with [[09-object-memory-model]],
this shows that erasing a Rust call or `RefCell` check in isolation does not remove the
dominant computed-access cost. The next array change must alter the connector itself:
stable backing-store metadata and direct AOT loads/stores, or a coarser native region
that keeps values unboxed across multiple operations.
