# 219 — The JS-to-native lowering functor: explicit target category and identity-component inventory

Status: planned

[[207-category-theory-literature-grounding]] audits individual tasks' categorical
claims. This task does the thing all of them are in service of: state the actual functor
this whole project computes — `L : JS -> Native` from a category of JS program
fragments (objects = types/shapes of runtime value, morphisms = JS operations) to a
category of native execution (objects = C-like data layouts — flat structs, contiguous
arrays, raw function pointers/labels; morphisms = straight-line machine code and direct
jumps/calls) — and use it to answer the concrete question underlying this whole research
thread: for which JS constructs is `L` already (isomorphic to) the identity embedding —
meaning execution is already exactly what a C program doing the same thing would do —
and for which is `L` a lossy embedding that must carry extra structure (a tag, an
indirection, a runtime check) the target category has no native counterpart for?

This reframes "what's slow" as "where does `L` fail to be full/faithful," which is a
sharper question than a profile alone gives: a profile shows *where* time goes; this
shows *why* it structurally must, given the source and target categories as they
currently stand, versus where it's merely an unimplemented but achievable identity.

Concrete inventory (fill in as the audit proceeds; each row should end at either "L is
identity here, done" or a pointer to the specific planned task closing the gap):

1. **Numbers.** A JS number used only through guarded arithmetic ([[10]], [[19]]) already
   lowers to a raw machine float/int — `L` is identity here once [[10]] lands. State
   the guard as exactly the non-identity part: `L` is identity *conditional on* a guard
   passing, i.e. `L` is a partial functor and the guard is the totality check, not
   incidental scaffolding — this reframing is itself useful: a guard is not overhead
   *added to* an otherwise-native operation, it *is* the proof obligation that makes the
   native operation licensed at all.
2. **Property access on a stable shape.** Once [[08]]/[[135]]/[[150]] apply, a
   `.` access lowers to a fixed struct-field offset load — again `L` is identity
   conditional on the shape guard. A *dictionary-mode*/megamorphic object
   ([[159]], [[200]]) is where `L` is genuinely not an identity embedding: JS's
   fully-dynamic property model has no native counterpart, and the honest target
   category object for it is a hash table, not a struct — `L`'s image for this case
   should be stated as "hash-table lookup," not judged a failure to reach struct-offset
   speed it structurally cannot reach.
3. **Closures.** [[41-flattened-closure-capture]] wants closures to lower to a flat
   capture struct plus a function pointer — a C-like closure representation
   (environment pointer + code pointer, as in a standard closure-conversion result). Where
   this is achievable (statically-determined capture set), `L` is close to identity;
   where capture is dynamic (`eval`, `with` — mostly excluded already), it is not, and
   that's a real semantic gap, not a missed optimization.
4. **Exceptions/control flow.** [[71]]/[[126]] model exceptions as an effect; state
   whether their target-category image is zero-cost (native unwind-table-style, no
   per-try-block overhead on the non-throwing path, matching what C++/Rust's
   zero-cost-exceptions model achieves) or whether it still carries a runtime check on
   the non-throwing path — if the latter, `L` is not yet identity on this construct and
   the gap is concrete and closeable.
5. **GC/allocation.** No native C program has an analog for tracing GC; state this
   explicitly as a construct where `L`'s target-category object is *irreducibly*
   different (a managed heap cell, not a stack/static allocation) except where escape
   analysis ([[24]], [[33]], [[176]]) proves the value nonescaping and can retarget it to
   a genuinely native (stack/register) object instead.

This task does not implement anything; it produces the inventory and, for every row
found to be a genuine (not-yet-closed) gap, a citation to the existing planned task
responsible for closing it, or a new task if none exists.

Acceptance: the five categories above (and any others found during the audit) are each
resolved to one of: "L is identity, verified by disassembly matching hand-written C for
an equivalent operation," "L is identity conditional on a guard, guard is the proof
obligation," or "L is irreducibly non-identity here, here is why, here is the task
closing the gap as far as it can go"; no row is left as an unexplained "this is just
slow."

Primary source: this task is the concrete instantiation of Conal Elliott's
*Compiling to Categories* framing already cited in [[207]] — `L` here is exactly the
cartesian-closed functor that paper's method asks you to state explicitly rather than
leave implicit in a compiler's ad hoc lowering code.
