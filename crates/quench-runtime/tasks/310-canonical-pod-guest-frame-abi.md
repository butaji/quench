# 310 — Canonical POD guest-frame ABI and explicit owned result word

Status: complete

Make the AOT stencil view and the Rust runtime view derive their common guest-frame
prefix from one immutable schema instead of maintaining matching structs and numeric
offsets by hand.

`stencil-aot/guest_frame_schema.rs` now defines the complete thirteen-field prefix,
callback aliases, named word offsets, and compile-time size/alignment/field-offset
proofs. `src/dynjit.rs` instantiates the runtime view with `Value`, `InlineSite`,
`RegionArrayView`, and callbacks whose owner remains the complete `DynFrame`.
`stencil-aot/handlers.rs` instantiates the AOT view with `RawValue`, its POD site/view
records, and the nominal raw callback owner. The two pointer types have the same C ABI;
callbacks deliberately still receive the complete runtime frame because slow kernels
need its Rust sidecar.

`DynFrame` is now an offset-zero `GuestFrameHeader` followed by a `DynFrameSidecar`.
The header result is an owned `RawValue` word, with explicit replace/take/drop
transitions. `Value::{as_borrowed_raw, into_owned_raw, from_owned_raw, from_raw_ref}`
make the ownership boundary named rather than relying on representation casts. Tests
prove that immediate bits survive and String/Function/RegExp owners transfer and drop
exactly once.

This is a representation prerequisite, not a call-speed optimization. The ordinary
call edge still creates and completes the Rust sidecar and still enters
`execute_direct_call`. Alternating complete-suite measurement against Task 307 confirms
that result: aggregate `2054.04 -> 2047.02` (**-0.34%**, neutral/noisy), with raw data in
`reports/task310-pod-guest-frame-ab-3`. The frozen candidate is
`/tmp/deegen-task310-pod-guest-frame`, SHA-256
`e335b925a4675dc2e308b12f0150d768eb0e4d73b6829fb2f0c4dc66d78625f1`.

Verification:

- normal release suite: 111 passed;
- `DEEGEN_OBJECT_GC_STRESS=1` release suite: 111 passed;
- focused owned-result stress test passes;
- AOT and runtime layout assertions compile from the same schema;
- all offsets and word counts are named constants; no layout magic numbers were added.

Acceptance: complete. The canonical ABI is now one quoted fact used by both lowering
worlds, the effectful ownership transition is confined to explicit boundary methods,
and the neutral A/B result is recorded without claiming a score improvement. The next
performance slice remains Task 146/181's actual pointer-bump activation and direct
continuation transfer.
