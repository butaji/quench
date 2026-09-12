# 290 — Partial prefix inlining: absorb the common case, call for the rest

Status: planned

Distinct from both [[20-inline-via-node-composition]] (whole-callee inlining as deferred
node composition) and [[163-caller-customized-stencil-images]] (a whole specialized
callee image per bounded caller context): neither currently states the narrower,
often-cheaper middle option of inlining only a callee's *common-case prefix* — the
guard-and-fast-path portion — directly into the caller's region, leaving the callee's
rare/generic tail as a real out-of-line call reached only when the inlined prefix's
guard fails. This is cheaper than [[20]]'s full-callee inlining (less code duplicated
into every caller) and more broadly applicable than [[163]]'s whole-image
specialization (works even when the callee's rare tail is large or itself calls
further, which would make a full specialized image expensive to duplicate per caller
context).

Concrete target: any small helper whose body is dominated by one guard followed by a
short fast return, with a longer, rarer fallback after — several of this project's own
already-established guard/kernel patterns fit this shape (a property-access helper that
is "check shape, return slot value" in the common case with a longer generic-lookup
fallback; a numeric operation that is "check both operands are the guarded numeric
representation, do the op" with a longer coercion fallback). Absorbing exactly the
guard-and-fast-return prefix into the caller's own stencil region removes the call/frame
overhead ([[251]]'s register-allocation-ceiling concern) for the common case
specifically, while the rare-case call remains a real, unduplicated call — avoiding
[[20]]'s full-body code-growth cost for the part of the callee that rarely matters.

Concrete steps:
1. Identify a callee already used as a guarded helper from multiple call sites where the
   guard-and-fast-path prefix is short and the fallback tail is comparatively large or
   itself effectful (a poor full-inlining candidate under [[20]]'s existing scope, but a
   good partial-inlining candidate).
2. Compile the caller's call site as: the prefix's guard and fast-path fused directly
   into the caller's region (no call, no frame, matching [[251]]'s register-allocation
   argument), branching to a real call to the callee's remaining tail only on guard
   failure — the callee itself remains a normal, separately-compiled function reachable
   from elsewhere, not duplicated or specialized per caller.
3. State the callee-tail-reachability invariant explicitly: the un-inlined tail must
   remain independently correct and callable (from this site's fallback branch, and from
   any other call site that did not get this treatment), since it is not being
   duplicated or specialized, only bypassed in the common case.

Acceptance: at least one identified guarded-helper call site compiles with its
guard-and-fast-path prefix fused directly into the caller (no call/frame overhead on the
fast path, verified by disassembly), while the rare-case tail remains a real,
unduplicated call reachable from the fallback branch; correctness holds for both the
fast path and the rare-tail path, including from call sites that were *not* given this
treatment (confirming the tail's independent correctness); alternating A/B on the
affected call site(s) shows a measured gain from the removed call/frame overhead with no
correctness regression across the full V8v7 suite.

No new external citation needed beyond this project's own existing composition
machinery ([[01]], [[20]], [[128]]) — this task states a narrower, previously-unstated
point on the same inlining spectrum those tasks already occupy.
