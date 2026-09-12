# 46 — Closed-world program mode and whole-program colimit pass

Status: planned

The whole-program optimization advantage (global shape unification, cross-module hash-consing, whole-call-graph inlining) is only sound when the program is closed: no `eval`, no dynamically injected code, no code arriving after the initial link pass. Make this an explicit, checked mode rather than an implicit assumption.

Concrete steps:
1. Add a `DEEGEN_CLOSED_WORLD=1` mode (naming consistent with existing `DEEGEN_*` env vars) that statically rejects or falls back to per-function analysis when `eval`, `Function` constructor, or dynamic `load()` calls are present in the source.
2. Under closed-world mode, run a whole-program pass computing a single shape lattice colimit across all call sites (feeding [[07-hidden-classes]]/[[08-property-inline-caches]]) instead of per-site incremental IC learning.
3. Wire [[42-whole-program-hash-consing]] and a whole-call-graph variant of [[20-inline-via-node-composition]] to run only in this mode, since both require the closed-world guarantee to stay a general (not benchmark-shaped) optimization per [[04-bytecode-coverage-map]]'s constraint.

Acceptance: closed-world mode is correctly rejected (falls back cleanly) for any program using `eval`/dynamic `load`; a closed-world program shows measured gains from whole-program shape unification and hash-consing over the equivalent per-function incremental path; the fallback path's behavior is unchanged from today's default mode.

Prerequisite gate: [[07-hidden-classes]] must reach a *clean* complete state before step 2 begins — not merely `complete` in name. As of this writing 07's own status reports an aggregate −0.81% regression and explicitly unresolved function-owned-property handling; a whole-program shape colimit built on an unstable or incomplete shape system will compound that instability across every call site instead of one. Do not start step 2 until 07's regression is resolved and function-owned properties are handled.

Theoretical grounding: this is the first Futamura projection — specializing an
interpreter to a fixed source program yields an executable — but performed statically
ahead of time over the whole closed program rather than at runtime over one function.
Graal is the one production system that performs the first projection directly (via
partial evaluation of its AST interpreter); PyPy instead traces hot loops at runtime,
and V8/SpiderMonkey/JSC specialize incrementally per call site as they execute. This
project's whole-stencil-image-from-first-use design is closer to Graal's ahead-of-time
partial evaluation than to any of the tracing/incremental designs — which is the
structural reason whole-program passes (this task, [[42]], whole-call-graph [[20]]) are
sound here in a way they would not be for an incrementally-specializing engine: nothing
downstream depends on runtime profile feedback that a whole-program pass could
invalidate.

Source: Futamura projections practical framing and Graal's first-projection
implementation: <https://dl.acm.org/doi/10.1145/3359061.3361077>.
