# 216 — Kleisli-category soundness proof for effect-reordering optimizations

Status: planned

[[21-loop-invariant-hoist-associativity]] hoists loop-invariant computation above a
loop, [[47-compile-time-guard-elision]] deletes guards outright, and
[[71-algebraic-effects-control-flow]] plans an algebraic-effect representation for
exceptions/generators/async. Every one of these is, underneath, a claim that moving or
removing a computation relative to an effectful boundary preserves program behavior.
That claim is exactly what Kleisli-category law-checking exists to make precise instead
of case-by-case judgment calls: if JS execution is modeled as a Kleisli arrow
`a -> M b` for the effect monad `M` (heap mutation, exception, I/O — whatever
[[71]]/[[173-effect-token-memory-ssa]] settle on as the effect algebra), then Kleisli
composition is associative and has identity by construction, and a reordering is sound
exactly when it commutes with `M`'s bind in the specific way the transformation claims —
not merely "seems obviously fine because the value is pure."

This formalizes and tightens a distinction the codebase already draws informally: pure
loads/guards may float freely, while a store, call, or throw is a hard ordering barrier.
Concretely:
1. State the effect monad `M` explicit enough to write Kleisli composition for it (this
   should reuse whatever [[173]]'s MemorySSA/effect-token model settles on rather than
   inventing a second effect representation).
2. State loop-invariant hoisting ([[21]]) as: a subcomputation `c` commutes past a loop
   body's effectful arrow `f : a -> M a` iff `c` is representable as a *pure* Kleisli
   arrow (`c >=> f = f' >=> c` for the hoisted `f'`) — this is precisely when `c` has no
   observable interaction with `M`'s effect, which is a checkable syntactic property
   given the effect-token model, not an ad hoc "looks invariant" check.
3. State guard elision ([[47]]) as: removing a guard arrow `g : a -> M a` (where `g`
   either returns its input unchanged or diverges via `M`'s failure case) is sound
   exactly when a prior arrow in the same Kleisli chain already factors through `g`'s
   success case — i.e. guard elision is arrow subsumption in the Kleisli category, not
   a separate ad hoc "we proved this shape is stable" argument per guard family.

Acceptance: the effect monad and its Kleisli composition are stated precisely enough
that "does transformation X commute" is a syntactic check against that definition, not a
judgment call; at least one existing or planned hoisting/elision transformation
([[21]] or [[47]]) is restated in these terms and gains a checked law test analogous to
[[49-law-tested-rewrite-gate]]'s existing discipline; the restated law rejects at least
one hypothetical unsound reordering (a store or throw incorrectly hoisted) as a negative
test, demonstrating the framework actually constrains something rather than only
describing what was already believed correct.

Primary sources:
- Kleisli category / monadic composition, standard category-theory-for-programmers
  reference: Moggi, *Notions of Computation and Monads* (Information and Computation,
  1991) — the original effect-as-monad formalization this task's arrows are drawn from.
- Wu & Schrijvers on effect-handler fusion under a similar composition-soundness lens
  (already surfaced via [[211]]'s codensity source): <https://okmij.org/ftp/Haskell/zseq.pdf>
