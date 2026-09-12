# 249 — Associative-scan monoid recognition: the general foundation behind carry/accumulator chains

Status: planned

**The foundation [[246]] is one instance of, not a standalone trick.** [[01-stencil-category-core]]
already proves associativity for `Stencil` composition, and [[64-commutative-parallel-composition]]
already licenses lock-free parallel composition wherever that associativity/commutativity
can be established — but both apply that law to *code* composition (which stencil runs
before which). [[246-carry-save-bignum-multiply-vectorization]] needs the exact same
law applied one level down, to a *data*-flow combining operator inside a loop body: the
carry-propagate step in `crypto.js`'s `am3` is, categorically, a monoid — a binary
operator with an identity element, satisfying associativity — and **any** loop computing
a running fold over a monoid operator, regardless of source shape, admits the same
parallel restructuring `am3` needs: Blelloch's work-efficient parallel-prefix scan
(*Prefix Sums and Their Applications*, CMU 1990), the general algorithm of which
carry-lookahead/carry-save addition is one hardware-level instance. Building this as a
general recognition mechanism, rather than a one-off `am3`-shaped rewrite, is the
actual "solid foundation" this session's per-benchmark findings ([[246]], and any future
carry/running-total/running-max shape found in later suites) should sit on.

Concrete steps:
1. State the general recognition rule: a loop body computing `acc = acc ⊕ x[i]` (or the
   two-variable carry/value form `am3` uses) is a scan candidate when `⊕` can be proven
   associative — reuse [[01]]'s existing category-law-testing discipline
   ([[49-law-tested-rewrite-gate]]) to check this the same way stencil composition's own
   associativity is checked, rather than trusting it by inspection per site.
2. Once proven, license the standard two-phase Blelloch restructuring generically: an
   up-sweep computing partial combines in parallel, a down-sweep (or, for the
   carry-specific case, one final sequential carry-propagate pass, which is the
   cheaper specialization [[246]] actually needs, not the full general scan) — implement
   both, since a general reusable mechanism needs the full scan for non-carry
   accumulator shapes ([[39]]'s reduce loops with a genuinely non-numeric-carry
   associative combiner, e.g. a running max/min or bitwise-or accumulator, if any such
   shape turns up in a later per-suite audit) and the cheaper specialization for the
   carry-chain shape specifically.
3. Re-point [[246]] at this task as its general mechanism: `am3`'s carry-save
   restructuring becomes the concrete, corpus-verified instance of step 2's carry-chain
   specialization, not a separately-invented rewrite.
4. State the non-associative rejection case explicitly and test it: a loop whose
   combining step is *not* associative (order-dependent, e.g. floating-point
   accumulation where reassociation would change rounding behavior) must **not** be
   recognized as a scan candidate — this is a real correctness boundary (reassociating
   floating-point addition changes results) distinct from the exact/integer carry case
   `am3` actually is, and getting this wrong would silently change numeric results,
   the same class of risk [[237]]'s adversarial catalog exists to catch for the guard-
   erasure family; add this as its own negative test rather than assuming integer-only
   scope protects against it.

Acceptance: the general associativity-proof-then-scan-restructuring rule is stated and
law-tested per [[49]]'s discipline; [[246]]'s `am3` carry-save rewrite is confirmed to be
this task's carry-chain specialization rather than a separate mechanism, with its
existing acceptance criteria unchanged; a floating-point running-accumulation loop is
confirmed correctly *rejected* by the associativity proof (negative test, per step 4),
preventing an unsound reassociation; if a genuine non-carry scan-shaped loop is found in
a later per-suite audit, it is recognized and restructured using this same general
mechanism without new bespoke recognition code, demonstrating the foundation actually
generalizes rather than only covering the one motivating case.

Primary sources:
- Blelloch, *Prefix Sums and Their Applications* (CMU-CS-90-190, 1990) — the standard
  work-efficient parallel-scan algorithm this task's general mechanism implements:
  referenced via the widely available summary at
  <https://developer.nvidia.com/gpugems/gpugems3/part-vi-gpu-computing/chapter-39-parallel-prefix-sum-scan-cuda>
  and <https://classes.engineering.wustl.edu/cse231/core/index.php/Scan>.
- [[01]]'s and [[64]]'s existing associativity/commutativity citations, reused rather
  than re-derived for this data-flow-level application of the same law.
