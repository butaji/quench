# 12 — Direct dense-array paths

Status: complete

Audit all indexed get/set operations and route dense numeric indices directly to array storage. Keep `length`, holes, extension, and prototype fallback explicit. Generic string properties must not force dense accesses through shape or hash lookup.

Acceptance: direct indexed load/store stencils or kernels, bounds/hole/length tests, and array-heavy benchmark A/B evidence.

Current state: computed numeric keys now take a direct dense-array path before property-key string conversion. Valid JavaScript array indices are checked as finite, non-negative integers no larger than `u32::MAX - 1`; loads read the `Vec<Value>` slot directly and stores resize/write it directly. Non-index numbers, strings, and non-array receivers retain the canonical property fallback. This removes the former number-to-`String` allocation followed immediately by `parse::<usize>()` on every indexed array access.

Measurement: `reports/array-index-ab/comparison.txt` records targeted alternating medians of Crypto 256→391 (+52.73%) and Navier–Stokes 362→636 (+75.69%), aggregate +63.81%. `reports/array-index-full-ab/comparison.txt` records the seven reliably bounded suites at 433.760→500.904 (+15.48% aggregate), with the other five suites between −0.06% and +1.76%. This optimization is accepted; the task remains in progress until prototype-chain behavior for absent indexed slots and direct executable stencil coverage are explicit.

Task 376 sharpens the remaining executable slice. Task 373 still sees `GetComputed` as
the first unsupported operation for 1,657,605 residual helper entries. A direct computed
family by itself is estimated to close 460,479 entries; together with already-planned
bitwise/numeric-unary cover it closes about 977,987. Add rustc/LLVM-cooked dense get/set
leaves with an AOT-visible array layout, numeric-index/integer/bounds/packed guards, and
canonical unexecuted-op slow edges. Do not wrap the existing Rust dense helper: the hit
must load/store the backing slot and continue inside the composed block.

Task 381 completes that executable slice. Every object now publishes a fixed-layout,
derived `{elements, length}` dense projection beside its shape/prototype header. The
rustc/LLVM-cooked `GetComputed` and `SetComputed` templates guard the object tag, numeric
integer index, dense backing, bounds, ownership class, and element representation-count
invariant before loading/storing the `Value` word and tail-transferring to the next
stencil. Missing indices and non-array receivers take the canonical slow edge, so
prototype lookup and array extension retain the existing semantics. The direct store only
writes existing slots whose number/non-number class is unchanged; other transitions use
`ArrayStorage::set`, preserving its derived `non_number_count` fact.

The nine-pair exact comparison in
`reports/task381-dense-computed-exact-ab-9/comparison.md` measures +1.57%, with a 95%
paired-bootstrap interval of `[+0.03%, +3.36%]`. All 144 release tests pass, including
executing both copied-and-patched dense leaves and their out-of-bounds slow edge. This
satisfies the remaining stencil and prototype-fallback acceptance conditions.
