# 293 — Separation logic's frame rule for proving heap-region disjointness

Status: planned

[[173-effect-token-memory-ssa]] models heap reads/writes as MemorySSA uses/defs to
support clobber queries and load/store elimination; [[292-monoidal-register-resource-allocation]]
needs a proof that two composed sub-regions' resources are disjoint before licensing
their tensor-product composition. Both currently rely on a general, unnamed
"independence" argument. Separation logic supplies the precise, standard tool for this
specific class of proof: its **frame rule** —
`{P} C {Q} ⟹ {P * R} C {Q * R}` — states that a command `C` proven correct for heap
footprint `P` remains correct when an arbitrary *disjoint* additional footprint `R` is
present alongside it, untouched, because the separating conjunction `*` asserts the two
footprints share no addresses. This is exactly the proof shape [[173]]'s clobber query
and [[292]]'s tensor-product law both need, stated with a real logic instead of an
informal "these don't alias" argument re-derived per site.

Concrete steps:
1. State the heap footprint of a representative stencil connector (a property-slot
   access via [[08]]/[[129]], a dense-array element access via [[32]]/[[99]]) as a
   separation-logic assertion over the specific addresses/offsets it touches, not merely
   "the object" as an opaque whole.
2. Restate [[173]]'s clobber-query soundness condition as an instance of the frame rule:
   a load can be forwarded across an intervening store exactly when the store's
   footprint and the load's footprint are provably separated (`*`), which is a sharper,
   checkable condition than a generic "no alias" heuristic.
3. Apply the same frame-rule argument to [[292]]'s register/resource tensor product:
   two composed sub-regions' resource sets are tensor-separable exactly when their
   respective heap/register footprints satisfy the same separating-conjunction
   condition — unifying the two tasks' independence proofs under one logic rather than
   two separately-argued notions of "independent."
4. Verify the frame rule's precondition explicitly at a chosen field-access site: two
   guarded property accesses on the *same* object but *different* fixed slots are
   separated (disjoint offsets within one allocation); two accesses through an aliasing
   reference (the same slot reached via two different variable names) are correctly
   *not* separated, and must not be treated as tensor-separable by [[292]] or
   load-forwarded by [[173]].

Acceptance: at least one connector's heap footprint is stated as an explicit separation-
logic assertion; [[173]]'s clobber-query condition and [[292]]'s tensor-separability
condition are both restated as instances of the frame rule and shown to agree with their
existing (informal) independence arguments on the same test cases; the aliasing negative
case in step 4 is confirmed correctly rejected by the frame-rule check, not silently
treated as separated; no runtime behavior changes from this task alone — it is a formal
grounding pass, like [[207]], whose acceptance is the correctness and precision of the
stated proof obligation, feeding [[173]] and [[292]]'s own implementation work rather
than replacing it.

Primary sources:
- Reynolds, *Separation Logic: A Logic for Shared Mutable Data Structures* (LICS 2002)
  — the original frame rule and separating-conjunction formulation.
- O'Hearn, Reynolds & Yang, *Local Reasoning about Programs that Alter Data Structures*
  (CSL 2001) — the local-reasoning motivation directly relevant to this task's
  per-connector footprint framing.
