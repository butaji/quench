# 223 — Abstracting Gradual Typing as the systematic guard/coercion derivation method

Status: planned

**The general cases of JS dynamism this project actually has to close**, surveyed
directly rather than left implicit in scattered per-construct tasks:

1. **Polymorphic property shape** (any object, any property) — [[07]]/[[08]]/[[135]].
2. **Polymorphic arithmetic operand type** (`+`, `<`, etc. accept number/string/object
   and coerce via `ToPrimitive`) — [[10]]/[[19]].
3. **Polymorphic call target and arity** (any value can be attempted as a callee;
   `arguments`/rest/default parameters make arity itself dynamic) — [[27]]/[[35]]/[[188]].
4. **Polymorphic element representation** (dense/holey/typed array element kinds,
   including holes reading as `undefined`) — [[32]].
5. **Dynamic `typeof`/`instanceof` tag tests** against an open or closed set of
   constructors — [[34]]/[[220]].
6. **Dynamic scope/binding resolution** (`eval`, `with`, global lookup with possible
   late declaration) — [[13]]/[[89]]/[[204]], explicitly excluded from the closed-world
   fast path by [[46]].
7. **Abstract (`==`) vs. strict (`===`) equality's coercion table** — its own dynamic
   semantics distinct from arithmetic coercion, not presently called out as a separate
   guard family anywhere in the task list.
8. **Accessor properties** (get/set can replace what looks like a plain data property at
   any time) — a dynamic-dispatch case not distinctly named in [[08]]/[[135]].

Every one of tasks 19, 25, 31, 34, 47, 60, and 73 independently designs a guard, a
coercion, or an elision argument for one of these cases. Each is correct in isolation,
but each is also a separate hand-derived design with its own soundness argument to get
right. **Abstracting Gradual Typing** (Garcia, Clark & Tanter, POPL 2016) is the
literature's answer to exactly this proliferation problem: starting from a Galois
connection between a language's precise (static) types and its gradual (dynamic) types,
*both* the static typing rules *and* the required runtime checks are derived
mechanically from the connection, rather than designed per-construct — and the result
satisfies the standard gradual-typing correctness criteria (conservative extension,
type/gradual guarantees) by construction, not by a separate per-case proof.

This reframes [[60-galois-connection-guard-elision]] from "an analysis that happens to
be shaped like a Galois connection" into the actual AGT machine: state a "precise" type
lattice for this VM's native-shaped fast paths (fixed shape, fixed numeric
representation, fixed arity, closed tag set) and a "gradual" lattice that is JS's full
dynamic value space, define the abstraction/concretization pair `(α, γ)` between them
once, and mechanically derive from it: which guard a given operation needs (the
concretization of the operation lifted to gradual types), when a guard is provably
redundant (subsumed by an already-established precise type — this becomes [[47]]'s
elision criterion, derived rather than hand-argued per case), and what the fallback
operation must be when the guard fails (the operation's meaning at the fully dynamic/`?`
type — this is the generic path every stencil family already falls back to, now
justified as *the* AGT-correct fallback, not merely *a* correct one someone wrote).

Concrete steps:
1. State the precise-type lattice and the gradual-type lattice for this VM's actual
   value space (reuse [[06]]'s `Value` representation and [[07]]'s shape lattice as the
   precise side).
2. Define `(α, γ)` and verify the Galois connection law (`α(c) ⊑ a ⟺ c ⊑ γ(a)`) holds
   for the lattices as stated — this is the same law [[207-category-theory-literature-grounding]]
   already asks [[60]] to check, now given a concrete, complete construction to check it
   against instead of an ad hoc guard-by-guard argument.
3. Re-derive at least two of the eight dynamism cases above (recommend: case 2
   arithmetic coercion, since [[10]]/[[19]] are still `planned`, and case 7 abstract
   equality, since it is not distinctly represented anywhere yet) from the Galois
   connection mechanically, and compare the derived guard/fallback against what
   [[10]]/[[19]] would otherwise hand-design — confirm they agree, or that the derived
   version is more precise/more complete.
4. For case 7 specifically (abstract equality): either fold it into [[10]]'s guarded
   arithmetic/comparison stencils as its own coercion table if AGT derives a
   sufficiently similar guard shape, or open a new task if it does not.

Acceptance: the precise/gradual lattices and `(α, γ)` are stated explicitly with the
Galois law checked as a test, not asserted; two dynamism cases are re-derived through
this machine and shown to agree with (or improve on) their existing hand-designed
counterparts; case 7 (abstract equality) has an explicit disposition (folded into an
existing task or newly opened) rather than remaining unaddressed; [[60]] and [[73]] are
updated to point at this task as their concrete construction rather than standing alone.

Primary sources:
- Garcia, Clark & Tanter, *Abstracting Gradual Typing* (POPL 2016): <https://www.cs.ubc.ca/~rxg/agt.pdf>
- Follow-up precision/completeness refinement: <https://dl.acm.org/doi/pdf/10.1145/3434342>
