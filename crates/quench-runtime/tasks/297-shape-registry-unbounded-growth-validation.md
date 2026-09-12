# 297 — Validate whether the shape hash-cons registry grows unboundedly

Status: planned

Companion validation experiment to [[250-rc-cycle-leak-validation-deltablue]], same
methodology, different subsystem. `ShapeRegistry` (`src/main.rs:437-470`) holds
`by_keys: HashMap<Vec<String>, Rc<Shape>>`, inserted into on every newly-observed
property-key sequence and never evicted from anywhere in the read code — there is no
weak-reference, LRU, or generational-collection path for a shape that stops being used.
This is the standard "keep hidden classes/maps alive forever" design real engines also
use, *but* production engines specifically call out weak references from optimized code
to hidden classes/maps as a distinct, deliberate mechanism (V8's own design explicitly
avoids pinning dead hidden classes indefinitely) — meaning this is a known failure mode
in the literature, not a hypothetical one, and this project's registry currently has no
corresponding mechanism at all.

**Concrete risk:** a program that dynamically constructs many structurally-distinct
objects with different property-key sequences (a plausible, not exotic, JS pattern —
object literals built from varying dynamic key sets, e.g. parsing loosely-structured
data) grows `by_keys` by one entry per distinct key sequence for the lifetime of the
process, with no bound. This is a different failure mode from [[250]]'s cycle-leak
concern (that one is about `Rc` reference cycles the collector can't reach at all; this
one is about a registry that is *reachable* and therefore not even a cycle-collector's
problem — it is simply an ever-growing table by design, unless a task explicitly bounds
it).

Concrete steps:
1. Confirm directly, by reading the current shape-interning code
   (`ShapeRegistry::intern`/`insert`, per [[135-shape-hash-consing]]'s implementation),
   whether any eviction, weak-reference, or generational-clearing path exists anywhere
   — do not assume either way from this task's description alone, re-verify against
   whatever the code looks like at the time this task starts, since [[135]] and related
   shape work are under active development.
2. Construct a synthetic workload that creates many (thousands+) structurally-distinct,
   short-lived object shapes in a loop, and measure `ShapeRegistry`'s table size and
   process memory growth directly, the same way [[250]] measures RSS for deltablue.
3. State the finding honestly: if the table grows unboundedly and this is confirmed
   consequential for a realistic workload (not just this project's own fixed 8-suite
   V8v7 corpus, which likely has a small, static, bounded set of shapes since none of
   the suites dynamically vary property-key sets at scale), open a bounded follow-up
   task (an LRU/generational eviction policy, or weak-reference-based collection tied to
   whatever heap/GC mechanism [[38]]/[[148]]/[[162]]/[[198]] eventually build) rather
   than leaving the finding unactionable.

Acceptance: a definitive, measured answer to whether `ShapeRegistry` grows unboundedly
under a shape-diverse synthetic workload; if it does, a quantified growth rate and an
honest statement of whether the V8v7 corpus itself is affected (likely not, given its
fixed and small shape population, per a direct check of the actual suite sources) versus
whether this is a real risk only for realistic, shape-diverse real-world programs; if a
bound is needed, a follow-up task is opened with a concrete eviction/collection
mechanism rather than the finding being left as an unaddressed note.

Source: `src/main.rs:437-470` (`ShapeRegistry` definition and `by_keys` insertion path,
this project's own source); no external citation needed beyond the standard "weak
references from optimized code to hidden classes" design point already established in
production engines (V8) as the known answer to this exact risk.
