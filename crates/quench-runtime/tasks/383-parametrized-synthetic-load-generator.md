# 383 — Adopt `quench-bench/micros` instead of building a new load generator

Status: planned

**Superseded premise.** This task originally proposed building a new parametrized JS
load generator from scratch. That was wrong: `../quench/quench-bench/micros/` already
exists as a mature, sibling-repository framework doing exactly this job, more
thoroughly than a from-scratch build would produce quickly. Do not duplicate it —
adopt it.

**What it already has**, read directly from `quench-bench/micros/README.md`: 24
experiment groups and 124 executable contrast variants (plus 100 preserved legacy
numbered workloads), covering exactly the categories this project cares about —
calls (inline/direct/changing/receiver/bound/multi-argument), locals, changing types,
allocation lifetimes, object/array access patterns, input-size scaling. Every run is
checked against a semantic oracle (undefined/−0/NaN/Infinity/BigInt/lone-surrogate
preserving result encoding). Timing uses balanced alternating engine order, independent
processes, and a `next` command that reports missing controls rather than claiming a
root cause from timing alone. Critically, `measure lifetime --variant cycles --lifecycle`
already does 120-epoch, three-process, plateau-detection memory-lifecycle measurement
with a `max(8 MiB, 5% of late-run median RSS)` growth allowance — this is a more
rigorous version of exactly what [[250-rc-cycle-leak-validation-deltablue]] needs, already
built and generalized beyond deltablue specifically.

The runner's `--engine`/`--bun`/`--oracle` flags accept **any executable path**, not
only quench's own binary — meaning this project's own `cargo run --release` binary (or
equivalent built artifact) can be passed directly as `--engine` with no adapter needed
beyond confirming it accepts plain-script execution the way the harness expects.

Concrete steps:
1. Confirm this project's release binary runs correctly as a `micros` `--engine` target:
   `node quench-bench/micros/run.mjs smoke --engine <path-to-deegen-binary>`.
2. Point [[250]] at `measure lifetime --variant cycles --lifecycle` directly instead of
   its own hand-rolled RSS-sampling steps — reuse the existing plateau-detection
   protocol rather than re-deriving one.
3. Check whether any of the 24 existing experiment groups already cover [[297]]'s
   shape-diversity concern (an `objects`-family group is the likely candidate given
   `--instrument sites` diagnoses object/property-site behavior); if a suitable group
   exists, point [[297]] at it directly; if none does, that is a genuine, narrower gap
   to fill *within* this existing framework (a new `cases/` group), not a reason to
   build a separate tool.
4. For the corpus gaps this session found by direct grep (zero `Array.shift`/`unshift`
   usage motivating [[225]]; the regex lookahead/backreference case motivating [[245]]),
   check whether an existing `calls`/`arrays`/`strings`-family group already exercises
   these, and add a case to the existing corpus if not — again as an addition to this
   framework, not a parallel one.
5. Confirm the `--trace-engine`/`--instrument counters|sites` diagnostic path (which
   requires an *already-built* trace-enabled binary, never building one itself) composes
   cleanly with [[253]]/[[254]]'s existing native-profiling work rather than
   duplicating it — these may turn out to be complementary (this tool giving
   correctness-checked micro-level contrast, [[253]] giving whole-suite native
   attribution) or one may supersede the other; state which, honestly, rather than
   running both without reconciling their roles.

Acceptance: this project's binary runs successfully as a `micros` `--engine` target;
[[250]] is repointed at `measure lifetime --variant cycles` and produces a result at
least as informative as its original ad hoc RSS-sampling plan; [[297]]'s shape-diversity
need is either matched to an existing experiment group or added as one new case within
this framework; the relationship between this tool's diagnostic instrumentation and
[[253]]/[[254]]'s native profiling is stated explicitly (complementary, redundant, or
superseding) rather than left as two parallel unreconciled efforts.

Source: `/Users/admin/Code/GitHub/quench/quench-bench/micros/README.md` (sibling
repository, read directly rather than assumed).
