# 164 — Canonical side-exit frame state

Status: planned

Represent the recoverable VM state at every speculative exit as immutable, structurally
shared `FrameState` data. Each live value records its semantic bytecode location, current
machine location, representation, and materialization recipe. Compile that description
to a stencil morphism `Materialize<SpecializedGamma, CanonicalGamma>` followed by a jump
to the correct general-purpose stencil continuation.

Frame states form a persistent parent/delta chain so attaching them to many guards does
not create quadratic metadata. Normalize and hash-cons equivalent states. The linker
shares materialization tails where profitable and may patch a guard directly to its
materializer. No exit enters an interpreter.

Categorically, materialization is the explicit natural-transformation component between
the specialized context functor and canonical generic-frame functor. In Lisp terms it is
quoted recovery data until the final copy-and-patch evaluation.

Acceptance: property tests compare exit-at-every-guard execution with the generic stencil
path; exception and ownership state are preserved; frame-state memory grows linearly;
Task 144 can keep values unboxed/register-resident across multiple ops and safely rejoin
generic stencils.

Sources: <https://v8.dev/blog/maglev> and
<https://webkit.org/blog/3362/introducing-the-webkit-ftl-jit/>.

Model every side exit as a terminal edge in specialized data flow. It consumes a
`FrameState`, materializes the canonical context, and leaves the specialized CFG; no
abstract facts from that exit merge into a normal successor or loop backedge. This is
the useful compiler property of deoptimization described by V8: exit blocks do not
produce liveness/load-elimination/escape-analysis facts for the next iteration. Source:
<https://v8.dev/blog/wasm-speculative-optimizations>.
