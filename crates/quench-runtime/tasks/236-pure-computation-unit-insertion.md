# 236 — Pure sub-computation as a trivial unit insertion: zero effect-ordering barrier

Status: planned

Variation of [[231]]'s exactness criterion applied to [[216-kleisli-effect-ordering-soundness]]'s
effect monad rather than to shape/tag/call guards. In a Kleisli category for an effect
monad `M`, every pure value `a` embeds via the monad's unit `η : a → M a`. [[216]] already
distinguishes pure sub-computations (free to float across an effect boundary) from
effectful ones (a hard ordering barrier) as a *soundness* question — may this reordering
happen at all. This task asks the complementary *cost* question [[231]]/[[232]] raise
for guards: does a proven-pure sub-computation's embedding via `η` require any runtime
representation or check at all, or is it — like an exact guard's classifying map — a
compile-time-only fact that should vanish from the compiled stencil.

The answer, stated precisely: `η` is a *natural transformation* from the identity
functor to `M`; naturality is exactly the "no runtime choice involved" property — unlike
an effectful arrow, which must actually execute against `M`'s runtime representation
(the frame's effect token, per [[173-effect-token-memory-ssa]]), a pure value's `η`
embedding commutes with everything around it by naturality's own law, with no
represented effect state to thread, touch, or check. Concretely: a loop-invariant pure
expression hoisted per [[21]]/[[233]] should carry no effect-token/ordering marker at
all in the compiled stencil — not a cheap one, none — because it was never actually a
Kleisli arrow into `M` in the first place, only trivially embedded into it for
type-uniformity in the source language.

Concrete steps:
1. State which existing stencil connectors currently thread an effect-token/ordering
   marker through a value the compiler has already proven pure (per [[173]]'s
   MemorySSA facts or [[216]]'s Kleisli-purity check), and confirm whether that marker
   is dead weight today — a concrete audit, not a presumption.
2. Where a marker is confirmed dead weight, erase it: a pure value's representation in
   the compiled stencil carries zero effect-related bytes/instructions, distinguishing
   it structurally from an effectful value's representation rather than merely skipping
   a check on it at runtime.
3. Verify the naturality law explicitly as a test: reordering a proven-pure
   sub-computation relative to any surrounding effectful arrow must produce identical
   observable behavior for every test in the corpus, which is precisely naturality
   restated operationally.

Acceptance: a representative loop-invariant pure hoist ([[21]]/[[233]]) compiles with
zero effect-token instructions attached to the hoisted value, verified by disassembly,
distinct from [[216]]'s existing soundness check (which governs *whether* hoisting is
allowed, not what it costs once allowed); the naturality-law reordering test passes on
the full corpus; a value incorrectly classified as pure (a negative test — a value that
actually does interact with `M`) is caught by [[216]]'s existing soundness machinery,
confirming this task's cost-erasure never bypasses that check.

Primary sources: Moggi, *Notions of Computation and Monads* (already cited in [[216]])
for the unit/naturality structure this task's erasure argument rests on; no additional
primary source needed.
