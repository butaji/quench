# 398 — Bottom-up stage-gated optimization: master the simplest load before the next

Status: planned

**The methodology this task states explicitly, using [[387]]/[[388]]'s already-adopted
foundation.** Don't spread optimization effort across all eight V8v7 suites (each
exercising many mechanisms simultaneously, per [[15]]'s blended aggregate) and don't
default to tackling `full-system-closure`-shaped work ([[146]]/[[336]]'s call/frame
continuum, richards-shaped composition) before the *simpler* mechanisms underneath it
are already near their achievable ceiling. Build bottom-up: master
`deegen-curriculum`'s stage 1 (interpreter-dispatch — straight-line arithmetic, branch,
loop, call, exceptions) to near-Node/Bun parity before treating stage 2 (call-IC) as a
priority, master stage 2 before stage 3 (property-generic-IC), and so on through stage 9
(full-system-closure). A composed region built on top of an unsettled lower stage
inherits that stage's slack, so its own measured gain is smaller and less durable than
its task text implies — this is the same lesson [[301]]'s Tier 0 already states for the
generic-executor/frame boundaries specifically, generalized here to the curriculum's
full staged axis as an explicit, checkable sequencing rule rather than an implicit
priority judgment.

**Why "ground up from the simplest load" is the right order, not merely a nice
convention.** A mechanism exercised at stage 1 (e.g. straight-line arithmetic dispatch)
is also exercised, as a sub-component, inside every later stage's more complex cases —
stage 9's full-system-closure case still does arithmetic, still dispatches calls, still
reads properties. If stage 1's arithmetic dispatch is not yet near-optimal, that slack
is baked into every later stage's measured ratio too, and a task attacking stage 9
specifically cannot distinguish "this is a stage-9-specific problem" from "this is
stage-1's unclosed slack showing up again at a higher composition level" without first
confirming stage 1 is settled. This is a stronger, more precise statement of exactly
what burned [[338]]'s reverted candidate and [[381]]'s reverted catalog: both attacked a
composition-level or leaf-level target without first confirming the underlying simpler
mechanism was already tight, and both results were confounded by slack from elsewhere.

Concrete steps:
1. Use [[388]]'s per-stage headroom curve as the literal gate: define "near-optimal" for
   a stage as a stated, named ratio-to-Node/Bun threshold (not an unstated intuition,
   consistent with this project's "no unexplained numeric threshold" discipline) — e.g.
   within some explicitly justified percentage of the better comparator engine's time
   for that stage's cases.
2. Do not prioritize a task whose primary target is stage N+1 or higher while stage N's
   measured ratio (per [[388]]'s curve) remains below that stage's threshold, *unless*
   the task explicitly demonstrates (per [[00]]'s preflight discipline) that its target
   is a genuinely stage-N+1-specific mechanism with no dependency on stage N's slack —
   this is a strong default, not an absolute rule, and the exception must be argued
   explicitly each time, not assumed.
3. Re-run [[388]]'s headroom curve after any stage-N-targeted task lands, confirming the
   gate condition for stage N+1 is now met before treating stage N+1 work as unblocked —
   this is the same re-measurement discipline [[301]]'s review cadence already requires
   for its own tier boundaries, applied to the curriculum's stage axis specifically.
4. Cross-reference against [[301]]'s existing Tier 0-4 ordering: state explicitly how
   the two sequencing axes relate (curriculum stage 1-2 roughly corresponds to Tier 0's
   generic-executor/dispatch boundary; stage 9's full-system-closure roughly corresponds
   to Tier 3's corpus-specific suite work) so a contributor reading either document finds
   a consistent picture, not two independently-maintained orderings that could silently
   drift apart.

## A stage is not done at "attempted" — it is done at "threshold met"

The gate above governs when stage N+1 work may *start*. It is a distinct, stronger
requirement that stage N's own work does not count as finished merely because some
tasks targeting it were tried, landed, or exhausted the obviously-cheap candidates —
it is finished only when [[388]]'s headroom curve actually shows stage N's ratio-to-
Node/Bun at or above its named threshold. If the cheap candidates for stage N run out
before the threshold is met, that is not a signal to move on to stage N+1; it is a
signal that stage N needs a harder, more structural candidate (the same "isolated leaf
attempts keep failing, whole-region attempts keep winning" lesson [[301]] already
documents from [[338]]/[[362]]/[[381]]), and effort stays on stage N until one is found
and lands, or until the gap is diagnosed and explicitly recorded as blocked on a named,
specific cause outside this project's current control.

Record each stage's status explicitly in this task (or a per-stage sub-record) as one
of: `unmeasured` (no [[388]] baseline yet), `below-threshold, active` (work ongoing),
`below-threshold, blocked` (a specific, named blocker prevents further progress right
now — not "we ran out of ideas"), or `threshold-met` (the only status that unblocks the
next stage by default per the gate above). A stage sitting at `below-threshold, active`
with no task in flight against it for an extended period is itself a violation of this
task's intent and should trigger [[301]]'s review cadence.

Acceptance: a named, justified near-optimal threshold exists for at least stage 1; each
curriculum stage carries one of the four statuses above, current as of the last
[[388]] measurement, not stale; [[388]]'s headroom curve is checked against the
threshold before any stage-2-or-higher task is treated as top priority; no stage is
silently abandoned below threshold — a `below-threshold` stage is either `active` (work
genuinely in flight) or `blocked` (a specific cause is named), never left unstated; at
least one already-in-flight higher-stage task ([[146]]/[[336]]'s call continuum, or a
corpus-specific stencil task) is explicitly re-evaluated against this gate — either its
lower-stage dependencies are confirmed already at `threshold-met` (the task proceeds as
prioritized), or a genuine gap is found and stage-1/2 work is reprioritized ahead of it,
with the reasoning recorded either way; [[301]] and this task cross-reference each other
so the two sequencing views stay reconciled.

No external primary source needed beyond [[387]]/[[388]]'s existing foundation and
[[00]]/[[301]]'s existing sequencing discipline — this task states an explicit rule
connecting them, not a new external technique.
