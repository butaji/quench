# Sparse-array metadata cleanup at slot reuse

`Heap::alloc` used to remove an entry from `sparse_arrays` whenever it popped a
reusable heap slot. The heap's sweep path already removes that cell's sparse
metadata before adding the slot to `free`; allocation-time removal therefore
repeated lifecycle cleanup. The change deletes that redundant hash operation
and documents the slot-reuse invariant at the single heap `free.push` site.

The focused regression allocates a sparse array, collects it, reuses its exact
slot, and verifies that the new array has neither the old indexed value nor its
old sparse length. Test262 Array Stage 31 passed 3,081/3,081 and the full Wasm
suite passed 67,124/67,124. A forced-GC sparse-array reuse probe passed on both
Quench and Node; the retained inputs and exact commands are in
[`correctness.json`](correctness.json).

The final 11-pair production screen is a pinned, alternating Quench-only
comparison on macOS 26.5, Apple M4 arm64, with 16 GiB physical memory and 10
logical CPUs. Active and zero-iteration scripts are hash-recorded in
[`gate.json`](gate.json); binaries are pinned by SHA-256 there. The explicit
sparse-side-table case retired 0.546% fewer instructions (11/11 pairs), while
median cycles improved 0.675% with a 95% interval from -0.228% to +1.815% (8/11
pairs). Its allocation control improved 0.336% in median cycles (7/11). After
subtracting the control's per-pair movement, the median improvement was 0.317%
with a 95% interval from -0.702% to +1.802%. The cycle result is inconclusive,
so this record makes no speed claim.

Median maximum RSS rose 0.273% in the target case, within the 0.5% budget; the
allocation control showed a nearly identical 0.272% rise. The paired median RSS
changes were +131,072 bytes in the target and +16,384 bytes in the control,
which does not establish a change-caused increase. The separate Splay gate also
did not establish a Score or cycle effect: median Score changed -7.97% (95% CI
-22.86% to +7.75%, 4/11 wins), and cycles improved 0.878% (95% CI -1.923% to
+5.586%, 6/11). Its lower median RSS is not treated as causal.

The source simplification is retained for the heap lifecycle invariant, not as
a measured Task 61 performance win. The control avoids explicit large sparse
array setup but cannot prove the sparse side table is absent, since Node
bootstrap may initialize it. The corrected control interpretation and raw
pairs are preserved in the JSON record.
