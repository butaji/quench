# 09 — Remove Rc/RefCell cost from object hot paths

Status: in_progress

Move object/string/function lifetime to VM-owned storage so tagged values can be copied without `Rc` increments and decrements. Keep mutation behind explicit VM/object operations; avoid paying a dynamic `RefCell` borrow for each slot access. A tracing or arena-backed lifetime scheme must define roots, cycles, and reclamation before raw handles replace `Rc` globally.

The target ownership rule is stronger than "a cheaper `Rc`": values in stencil
registers, VM registers, local frames, and globals are precise roots and therefore do
not update per-object counts when copied. Only heap edges participate in write-barrier
bookkeeping. Deutsch--Bobrow deferred reference counting validates the value of omitting
stack/local count updates, but this VM should not build a zero-count-table collector as
an intermediate destination: Tasks 162 and 193 already provide the tracing/safepoint
machinery needed to reclaim cycles. The reusable lesson is uncounted precise roots, not
the legacy reclamation policy.

Safe intermediate improvement: batch a borrow across a multi-step slow-path operation. This is not the desired hot-path representation.

Acceptance: ownership stress tests, cyclic-object behavior documented/tested, and profiles showing refcount/borrow routines absent from property fast paths.

Current state: borrowed tagged-pointer accessors remove temporary `Rc` clones from function dispatch. Static and computed property execution now borrow receiver/key values directly from the register file instead of retaining and releasing their heap pointers around every access; result values and stored heap values still take the ownership clone required by the current `Rc` representation. `RefCell` and owning result/source clones remain. `reports/borrowed-heap-ab/comparison.txt` records +0.63% aggregate over the initial shape implementation. The broader receiver-borrow change is accepted by `reports/borrowed-property-ab/comparison.txt`: seven-suite aggregate 491.540→505.313 (+2.80%), with Richards +5.06%, DeltaBlue +5.81%, and Splay +9.77%; Crypto is −4.20%, within the standing per-suite floor.

Rejected call-path experiment: borrowing the callee directly from the register file removed one function `Rc` retain/release, but `reports/borrowed-callee-ab/comparison.txt` measured −1.22% aggregate, Crypto −5.90%, and Navier–Stokes −7.05%. It was reverted; mutable VM dispatch while retaining an alias into the register frame likely inhibited optimization enough to outweigh the saved refcount. Future removal should use VM-owned handles or a call-site target cache rather than borrowing across dispatch.

Rejected release-only borrow-check removal: an `UnsafeCell<Object>` wrapper retained
`RefCell`-equivalent dynamic validation in debug builds and erased it in release builds,
without changing `Rc` ownership or object layout. Debug and release tests passed and the
complete smoke remained correct, but the four-repetition alternating A/B in
`reports/object-cell-ab/comparison.txt` measured 568.182→560.626 (-1.33%), with Crypto
at -5.19%. The implementation was reverted. This isolates the important conclusion:
the borrow flag alone is not the bottleneck; a future object-memory change must remove
or bypass the owning `Rc`/Rust-helper boundary as a whole, preferably through a stable
raw object/element connector consumable directly by AOT stencils.
