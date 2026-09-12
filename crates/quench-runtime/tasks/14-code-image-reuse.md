# 14 — Function image and linked stencil reuse

Status: in_progress

Audit `jit_cache`, `numeric_jit_cache`, closure creation, and `then_kernel` composition. A function identity should link one reusable image when its patch environment is identical. Immutable kernels consume one shared executable mapping; an already-patched `StencilInstance` may be shared by reference when all external obligations match.

Acceptance: counters distinguish template selection, link, cache hit, and execution; repeated closure creation does not relink identical images; patch-context differences cannot reuse an invalid instance.

Rejected stable-handle experiment: each function's `RefCell<Option<Rc<DynJitCode>>>`
was replaced with a copyable raw `CodeHandle`, while the VM identity cache remained the
sole owning root. All 61 tests and the complete smoke passed. The focused six-run
Richards/DeltaBlue comparison was only +0.26%. The noisy complete six-run comparison in
`reports/task14-code-handle-full-ab-6/comparison.txt` measured aggregate −0.37%, with
RayTrace −7.32%, Earley-Boyer −7.57%, and Splay −13.78%, violating the component floor.
The source change was reverted and the candidate preserved at
`/tmp/deegen-task14-code-handle`. Per-call image borrowing/refcounting is therefore not
the next bottleneck; do not retry this representation without stronger evidence.

Rejected experiment: dispatch borrowed the already-linked numeric/dynamic image from its function `RefCell` instead of cloning the image `Rc` on every execution. Although recursion permits multiple immutable borrows, `reports/borrowed-code-image-ab/comparison.txt` measured only +0.18% aggregate and RayTrace −9.47%, violating the standing per-suite gate. It was reverted. Image identity/relink reuse remains intact; removing the per-call retain should wait for a cache/handle representation that does not hold a dynamic borrow across execution.

Task 140 supplies that stable representation at the correct scope: each bytecode call
site owns a write-once monomorphic record containing the shared image and environment.
Matching calls borrow the cached image without holding a `RefCell` borrow across
execution, while mismatches use the canonical dispatcher. Its six-run confirmation is
+0.86%. Function-identity linking remains one image per cache key; broader accounting
and patch-context identity coverage keep this task open.
