# 250 — Validate whether deltablue's bidirectional constraint graph leaks memory

Status: planned

**Architectural-flaw hypothesis:** the current object memory model is `Rc<RefCell<>>`-
based (per [[09-object-memory-model]]'s own framing, still `in_progress`), and reference-
counting alone cannot reclaim a cycle — [[38-cycle-aware-memory-reclamation]] is the
task that would fix this, and it is still `planned`, not implemented. The V8v7 accepted
score is computed across all eight suites' correctness/speed, but correctness testing
has never (per a search of the existing 249-task ledger) checked whether memory is
actually *reclaimed* between benchmark iterations — only whether output values are
right and how fast they're produced. If any suite builds a genuine reference cycle, this
VM may be silently leaking unbounded memory every iteration that suite runs, with
nothing in the current acceptance-gate methodology positioned to catch it.

**This is not hypothetical for this corpus.** `deltablue.js`'s constraint-satisfaction
graph is a textbook bidirectional-reference structure: `Variable.prototype.constraints`
holds a collection of `Constraint` objects (`deltablue.js:539,551-552`), and constraints
hold direct references back to the variables they connect
(`this.myOutput.addConstraint(this)` at `deltablue.js:225`; `this.v1.addConstraint(this)`,
`this.v2.addConstraint(this)` at `deltablue.js:382-383`) — a `Variable` holding
`Rc<Constraint>` while that same `Constraint` holds `Rc<Variable>` pointing back is
exactly the shape `Rc` alone cannot collect. Whether the benchmark's own teardown code
explicitly breaks these cycles (some `Planner`/`Constraint` implementations do call an
explicit `destroyConstraint`/unlink step) or whether they are simply abandoned at the
end of each `deltablueRun` and leaked is an open, testable, currently-unanswered
question about this project's actual memory behavior on real corpus code — not a design
choice anyone has verified either way.

Concrete steps:
1. Read `deltablue.js`'s actual teardown path (does `Planner`/`Constraint` explicitly
   null out or unlink `myOutput`/`v1`/`v2`/`constraints` references anywhere, or does the
   whole graph go out of scope with cycles intact?) and state definitively whether the
   source *itself* creates cycles that survive to the end of one benchmark iteration —
   do not assume; read the actual teardown code before concluding either way.
2. Run deltablue for many iterations under this VM with RSS (resident memory) sampled
   between iterations, and directly measure whether memory grows unboundedly,
   plateaus (cycles broken/collected some other way), or grows and is bounded by some
   other mechanism (e.g. process-level allocator reuse masking a logical leak that would
   still matter for a genuinely long-running program).
3. If a leak is confirmed, state its severity honestly: does it affect the V8v7 score
   itself (a short benchmark run may never accumulate enough leaked memory to matter for
   the scored metric) even though it would be fatal for any long-running real program
   using a similar graph-shaped data structure — this distinction matters for how
   urgently [[38]] needs to move, and should not be conflated with "the score is fine so
   this doesn't matter."

Acceptance: a definitive answer, backed by direct measurement (not inference), to
whether deltablue's constraint graph currently leaks under this VM's `Rc`-based memory
model; if it leaks, a quantified growth rate (bytes leaked per iteration) and an honest
statement of whether this is currently invisible to [[15]]'s acceptance gate; if it does
not leak, a documented explanation of the specific mechanism (explicit unlink in source,
or some other reason) so this is a known, verified fact rather than an untested
assumption going forward; [[38]]'s priority is explicitly reassessed against this
result — a confirmed active leak on real corpus code is stronger motivation than the
task's current text reflects.

Source: `/private/tmp/js-engine-benchmark/v8-v7/deltablue.js:225,382-383,539,551-552`
(local V8v7 corpus checkout).

## Tooling update: use the existing lifecycle harness, do not build a new one

`../quench/quench-bench/micros` (sibling repository) already provides
`measure lifetime --variant cycles --lifecycle`: 120-epoch, three-independent-process,
plateau-detection RSS measurement with a `max(8 MiB, 5% of late-run median RSS)` growth
allowance, generalized beyond deltablue specifically. Use this directly for step 2
instead of a hand-rolled RSS-sampling script — see [[383]] for the adoption task
covering this project's binary as a `micros` `--engine` target.
