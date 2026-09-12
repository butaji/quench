# 231 — Exact-abstraction guard erasure: the precise criterion for zero-check O(1)

Status: planned

Every guard task built so far — [[08]], [[25]], [[31]], [[47]], [[60]], [[220]],
[[229]] — makes a fast path O(1) by replacing an expensive check (prototype-chain walk,
`instanceof`, content comparison) with a *cheap* one (a shape-pointer compare, a
protector-cell load). That is O(1)-with-a-check. It is not the strongest thing category
theory offers here. The strongest thing is a precise, checkable *criterion* for when the
check itself is redundant — provably always true at that program point — and can be
deleted from the compiled stencil entirely, leaving true zero-check execution: not a
cheaper guard, no guard.

**The precise criterion, from Cousot's own abstract-interpretation literature.** A
Galois connection `(α, γ)` between a concrete domain and an abstract domain is generally
*lossy*: `γ ∘ α` is extensive (abstracting then concretizing can only widen, never
exactly recover the original), which is exactly why a guard is needed — the abstract
answer ("this shape/type holds") is a sound over-approximation, not a certainty, so it
must be checked against the concrete value at runtime. But a connection can be an
**exact abstraction** — Cousot's own term, from *Types as Abstract Interpretations*
(POPL 1997), for the case where `γ` is injective and `α` is a left inverse of `γ` (a
*Galois insertion* specifically, where `α ∘ γ = id` exactly, not merely `⊑`). At a
program point where the applicable abstraction is exact for the specific concrete domain
reachable there, the abstract answer is not an approximation — it is provably the exact
answer, with no possible false positive, and the runtime check computing it is
redundant: its result is a compile-time constant.

**Where this actually applies in this codebase**, stated concretely rather than left
abstract:
1. A property access on an object literal whose complete construction the compiler has
   seen in full (no external mutation reachable before the access, per
   [[222-constructors-as-product-introduction]]'s totality proof) has an exact shape
   abstraction: the shape *is* the literal's field set, not merely "probably is." The
   shape guard at such an access is checking a compile-time constant and should not
   exist in the compiled stencil at all — this is strictly stronger than [[47]]'s
   "provably stable binding" elision, because [[47]] still frames the result as
   "elide a guard that would always pass" (a runtime fact discovered and then trusted),
   whereas exactness is a static, closed-form property of the abstraction itself,
   checkable independent of any particular run.
2. A closed-coproduct tag test ([[220]]) where the compiler has proven the coproduct
   closed *and* proven every value reaching the test site was constructed by exactly one
   already-known injection (not merely "one of finitely many, checked at the guard") has
   an exact abstraction for the same reason: there is nothing left to distinguish at
   runtime.
3. [[69-supercompilation-colimit]]'s whole-call-graph colimit, when its universal
   property gives a *singleton* fiber over a call site (every path reaching that site
   factors through the same unique shape), is exactness stated in colimit language: the
   colimit's uniqueness *is* `α ∘ γ = id` for that site's abstraction.

**Where it does not apply, stated equally concretely, to avoid overclaiming:** any site
[[229-protector-cell-invariant-flags]] exists to guard (a builtin, a shared prototype
reachable from more than one call site) is, by construction, not exact — the whole point
of a protector cell is that the abstraction *can* become stale, which is definitionally
inexact. Exactness and protector-cell-checked-O(1) are the two different, non-competing
answers to two different situations, not two implementations of the same idea; conflating
them would silently reintroduce a correctness bug (deleting a check that a protector
cell exists specifically because it sometimes must fire).

**Concrete implementation mechanism, so this pays off as real zero-runtime-cost, not
just a nicer proof.** Once a site is proven exact, represent the proof as a
zero-sized, compile-time-only Rust type (a `PhantomData`-carrying marker or const-generic
tag threaded through the relevant `StencilNode`/connector types) rather than as runtime
state — Rust's zero-cost-abstraction guarantee means a proof value that never appears at
runtime costs nothing in the compiled stencil, which is the concrete way "proven, not
checked" becomes an actual instruction-count reduction rather than a documentation
comment.

Concrete steps:
1. State the exact-abstraction criterion (`α ∘ γ = id`, Galois insertion) as a checkable
   property against [[60]]'s stated Galois connection from [[223]]'s AGT construction —
   this task is the completion of [[207]]'s audit item for [[60]], not a separate
   framework.
2. Identify the three concrete sites above (or others found during audit) where the
   criterion is checkable, and prove it holds — a test demonstrating no concrete value
   could reach that site without satisfying the abstraction, not an assertion.
3. Erase the corresponding guard from the compiled stencil at those sites via a
   zero-sized proof-carrying type, and verify by disassembly that no check instruction
   remains, not merely that the check got cheaper.

Acceptance: at least one site (recommend site 1, tied to [[222]]'s already-planned
totality proof) has its guard fully erased, verified by disassembling the linked
stencil and confirming zero guard instructions remain, not a cheaper guard;
[[229]]'s protector-cell sites are confirmed to remain checked (a negative test:
attempting to erroneously apply exactness there must fail the criterion, demonstrating
the boundary is real, not assumed); alternating A/B on the erased site(s) shows a
measured instruction-count and (where visible) score improvement with zero correctness
regression on the full V8v7 suite.

Primary sources:
- Cousot, *Types as Abstract Interpretations* (POPL 1997) — the exact-abstraction/Galois-insertion
  framing this task is built on: <https://pcousot.github.io/publications/Cousot-POPL97-p316-331-1997.pdf>
- Cousot & Cousot's original Galois-connection framework, for the general (inexact) case
  this task is contrasted against: <https://www.di.ens.fr/~cousot/publications.www/Top4-Abst-Int-1-PC-RC.pdf>
