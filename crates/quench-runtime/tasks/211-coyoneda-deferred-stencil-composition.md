# 211 — CoYoneda-embedded deferred composition for stencil transformations

Status: planned

[[17-seq-flatten-linear-link]] already wants linear-time flattening of composed
`Seq`/`StencilNode` values instead of naive repeated concatenation, and [[42]]/[[66]]
want a canonical normal form for the same structure. The CoYoneda trick generalizes this
from "flatten a free monoid efficiently" to "defer *any* structure-preserving
transformation efficiently," and is the actual mechanism the Haskell ecosystem uses in
production (`purescript-free`'s `Coyoneda`, van der Ploeg & Kiselyov's *Reflection
without Remorse*) to fix exactly the pathology this codebase is at risk of: a chain of
`n` transformations applied to a composed structure one at a time costs `O(n^2)` (or
worse) if each transformation walks/rebuilds the structure, because repeated left-nested
binds/maps over a free structure are the textbook case where naive representation is
asymptotically bad.

`Coyoneda f a = exists b. (b -> a, f b)` represents "apply this function to this
structure" *without actually applying it* — composing two coyoneda-wrapped
transformations is one function composition, O(1), regardless of how large the
underlying structure is or how many transformations have queued up. Codensity applies
the same idea one level up, to monadic bind chains specifically (relevant to
[[71-algebraic-effects-control-flow]]'s effect sequencing) rather than to a single
functor's `fmap`.

Audit every place `StencilNode`/`Stencil<In,Out>` composition currently applies a
rewrite, guard-insertion, or relocation-shift transformation eagerly to an already-built
structure rather than deferring it: [[18-identity-erasure-peephole]]'s peephole pass,
[[21-loop-invariant-hoist-associativity]]'s hoisting rewrite, and any pass that walks a
composed stencil more than once because each of several transformations does its own
full walk. Where a chain of such transformations is applied to the same value before
final linking, wrap the pending transformation in a coyoneda-style deferred functor and
fuse the whole chain into one walk at forcing time (link time), rather than one walk per
transformation.

Acceptance: at least one identified transformation chain (peephole + hoist, or
equivalent) is rewritten to defer via the coyoneda representation and fuse into one
walk; a benchmark with a long transformation chain applied to one large composed
stencil shows the walk count drop from O(chain length × structure size) to O(structure
size); output is verified byte-identical to the eager baseline on the full correctness
corpus; alternating A/B on compile-time-sensitive paths shows no regression.

Primary sources:
- van der Ploeg & Kiselyov, *Reflection without Remorse*: <https://okmij.org/ftp/Haskell/zseq.pdf>
- Coyoneda functor (`purescript-free`, reference implementation): <https://github.com/purescript/purescript-free>
- Codensity monad: <https://en.wikipedia.org/wiki/Codensity_monad>
