# 218 — Initial-algebra/final-coalgebra split for eager data vs. lazy codata

Status: planned

Arrays and plain objects on one hand, and iterators/generators on the other
([[88-iterator-protocol-fast-path]], [[71-algebraic-effects-control-flow]]'s generator
scope), are categorically different kinds of thing, and treating them uniformly is a
likely source of either missed fusion opportunities or unsound fusion attempts. A dense
array is (an approximation of) an *initial algebra* for a finite-list functor: it is
built up from a base case by finitely many constructor applications, so any
structure-consuming function over it is a catamorphism and total/terminating by
construction — this is exactly [[210-hylomorphism-lowering-fusion]]'s fusion
precondition. A generator/iterator, by contrast, is a *final coalgebra* instance
(codata): it is consumed by observation (`next()`), may be infinite, and its defining
operations are coinductive (an anamorphism/unfold), not inductive — fusing two
generator-chained operations is an anamorphism-anamorphism fusion, a different law with
a different (weaker, bisimulation-based) correctness argument than the catamorphism
fusion [[90-array-higher-order-fusion]] wants for guarded dense-array `map`/`filter`
chains.

Concrete payoff: [[90]]'s array higher-order fusion is currently scoped to guarded dense
arrays specifically, which is correct — it is implicitly relying on the initial-algebra
(finite, total) property to justify eagerly fusing the whole chain into one bounded
loop. [[88]]'s iterator fast path should *not* use the same fusion strategy for a
generator source, because a generator chain (e.g. a lazy `map`/`filter` over an infinite
or side-effecting generator) needs coinductive step-fusion (fuse one `next()` call
across stages, preserving the ability to stop after any element) rather than
catamorphism-style whole-structure fusion — attempting the array strategy on a generator
source would be unsound (it could force evaluation the source program never observes,
changing side-effect timing or looping forever on an infinite generator).

Concrete steps:
1. State the array/object case as the initial algebra for its shape functor explicitly,
   and confirm [[90]]'s existing fusion is exactly catamorphism fusion under that
   algebra (this should be a short confirmation given [[210]]'s general treatment, not
   new machinery).
2. State the generator/iterator case as the final coalgebra for its step functor
   (`next : S -> Option<(a, S)>`), and design [[88]]'s fused fast path as coinductive
   step fusion — composing the `next` functions of chained stages into one `next`, never
   materializing more than one element ahead, which is the correctness condition that
   distinguishes this from unsound eager fusion.
3. Document the boundary explicitly in both [[88]] and [[90]]: which source shapes take
   the initial-algebra (eager, whole-chain) fusion path and which take the
   final-coalgebra (coinductive, one-step) path, so a future contributor cannot
   accidentally apply the wrong fusion law to the wrong kind of source.

Acceptance: [[90]]'s fusion is confirmed sound as catamorphism fusion over the
initial-algebra structure with a stated law; [[88]]'s generator fast path (once built)
uses coinductive step fusion and is verified against a side-effecting generator test
(each stage's side effect must fire in the same order and cardinality as the unfused
protocol, including early termination via `break`) and against an infinite generator
consumed only partially (must not force more elements than observed); the two fusion
strategies are documented as distinct with the initial-algebra/final-coalgebra
distinction as the stated reason, not merely "arrays are fused, generators are not yet."

Primary sources:
- Initial algebras and final coalgebras, standard treatment: Jacobs & Rutten,
  *A Tutorial on (Co)Algebras and (Co)Induction* (EATCS Bulletin, 1997).
- Gibbons & Hutton, *Proof Methods for Corecursive Programs* (Fundamenta Informaticae,
  2005) — the bisimulation proof technique relevant to verifying coinductive step fusion.
