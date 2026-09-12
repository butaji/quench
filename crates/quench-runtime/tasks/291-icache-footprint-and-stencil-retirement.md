# 291 — Instruction-cache footprint as a tracked metric, with a standing stencil-retirement policy

Status: planned

Two related gaps this project's own growth exposes, neither addressed by
[[05-performance-harness]]'s existing correctness/speed measurement or
[[15]]'s score-only acceptance gate: total compiled code size (instruction-cache
footprint) is never tracked as its own metric, and there is no standing policy for
*retiring* an accepted stencil family whose measured value has become marginal relative
to the code-size/complexity it costs — only a policy (per [[00-optimization-routine]])
for rejecting a *new* candidate that fails its A/B gate. As the AOT stencil catalog
grows — 79's constant-operand variants, 163's caller-customized images, 79 numeric
condition families, each new task adding another compiled variant — total code size
grows monotonically with no corresponding check on whether every variant still earns its
share of icache pressure, and [[289]]'s cold-tail-merging work only addresses part of
this (shared cold tails, not whether a whole rarely-hit specialized fast path is still
worth its dedicated code).

This is a real risk specifically *because* of how this project is structured: unlike a
tiered engine that only keeps specializations actually observed as hot, this project
compiles every function to a stencil from first use with no hotness threshold — meaning
a stencil family that seemed valuable when accepted (passed its A/B on the suites active
at the time) could become a net-negative icache cost once enough *other* families have
also been added and the total working set no longer fits comfortably, without any
single task's own A/B ever catching the aggregate effect.

Concrete steps:
1. Add total compiled code size (per suite, and aggregate) as a tracked, reported metric
   alongside [[05]]'s existing score/correctness output — not a new benchmark, an
   additional column on the existing one.
2. Add an icache-pressure proxy metric (e.g. total working-set bytes touched per
   suite's dominant hot loop, or a simulated/measured L1-icache miss count if available
   on this host) so code-size growth's *actual* cost, not just its raw byte count, is
   visible.
3. Establish the retirement policy explicitly: periodically (or when total code size
   crosses a named threshold, consistent with this project's "no unexplained numeric
   threshold" discipline — name and justify the specific trigger) re-run an A/B with a
   candidate stencil family *disabled* against the current full catalog, not only
   against the catalog at the time it was originally accepted; if the aggregate score
   with it disabled is not measurably worse, retire it — remove the code, not just
   flag it as low-value.
4. Document at least one retirement decision (confirmed still valuable, or actually
   retired) as a worked example of the policy in practice, not merely a policy statement
   with no application.

Acceptance: code size and an icache-pressure proxy are tracked and reported alongside
every existing [[05]]-based measurement; the retirement policy is stated with a named,
justified threshold; at least one existing accepted stencil family is re-evaluated
under this policy against the *current* full catalog (not its original acceptance-time
baseline) with a documented outcome; if retired, the removal is verified to not regress
[[15]]'s aggregate score; if kept, the reason it still earns its code-size cost is
documented explicitly rather than assumed by default.

No external primary source needed — this operationalizes a policy gap in this project's
own existing measurement discipline ([[00]], [[05]], [[15]]).

## Round-forty-six fragment-explosion fixture

Add a synthetic repeated-bytecode function that would otherwise form many adjacent linked
fragments. Record final image bytes, fragment count, shared-kernel references, and collapsed
edges. The acceptance threshold is expressed as a named ratio to the smaller of primitive
composition and shared-kernel realization, not as an unexplained absolute byte count.

CPython's current copy-and-patch JIT provides a concrete external failure report: about 22
tail-linked traces for one 400-operation synthetic function produced roughly 18 MB of code
and a major slowdown. Use this only as the reason for the regression class; derive this VM's
limits from its own architecture and measurements:
<https://github.com/python/cpython/issues/149212>.
