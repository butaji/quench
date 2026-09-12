# 331 — Critical-path PMU optimization gate

Status: planned

Extend [[00-repeatable-optimization-routine]] with a pre-implementation performance
hypothesis and post-build hardware classification. For every candidate, state which
executed dependency chain is shortened: indirect control transfer, serial load chain,
call/frame transition, allocation/collection, or arithmetic latency. Instruction-count
or memory-access reduction alone is not a sufficient hypothesis.

On this Darwin/AArch64 host, add a reproducible `xctrace`/Instruments capture when the
available counters permit it, alongside the existing `/usr/bin/sample` and V8v7 A/B.
Normalize at least cycles, retired instructions, branch misses, instruction-cache events,
and data-cache events when exposed. If the host denies a counter, record it as unavailable
rather than substituting a magic estimate. Counter names and sample durations are named
constants in the script.

Acceptance: one retained and one rejected optimization are classified; the report explains
why their wall-clock result agrees or conflicts with count changes; Task 301 uses the result
to revise priorities. This gate guides experiments but never overrides the complete-suite
alternating A/B acceptance rule.

Primary-source motivation: a 2025 AOT JavaScript study found that a working dynamic-binary-
modification IC reduced memory accesses without reducing execution time on contemporary
processors: <https://arxiv.org/abs/2502.20547>. This project's neutral/rejected stencil
experiments make the same distinction operationally important.

Round-thirty-five refinement: normalize structural counts per guest bytecode or suite
iteration and always record residual-kernel entries, native/Rust transitions, helper calls,
IC outcomes, allocation/ownership operations, physical site updates, PC materializations,
copied instance bytes, unique kernel bytes, branches and direct fallthroughs. Publish linked
RX images to profiler symbol interfaces where supported; Linux uses the perf jitdump format
<https://github.com/torvalds/linux/blob/master/tools/perf/Documentation/jitdump-specification.txt>.
Use `llvm-mca` only on code already implicated by dynamic samples.

Screen with at least nine randomized paired runs and analyze paired log ratios; final score
acceptance belongs to [[365]]'s upstream-shaped same-process lane. If the score changes but
the predicted structural counter does not, record the result but do not claim the proposed
mechanism caused it.

## Counterfactual ranking before implementation

For shared kernels or native/Rust seams implicated by samples, add a counterfactual
experiment: virtually accelerate one named region and estimate end-to-end sensitivity
before rewriting it. Prefer a supported causal profiler when available; otherwise use a
controlled delay-balancing harness only for coarse ranking and label it an estimate. This
does not replace PMU classification or paired V8v7 A/B. It prevents spending a task on a
locally expensive region whose removal cannot move the whole program because another
serial dependency dominates.

Coz is the primary-source algorithm: it measures the effect of a virtual speedup instead
of only attributing samples, turning profiler output into an optimization prediction
<https://arxiv.org/abs/1608.03676>. Its platform permissions and sampling limitations must
be recorded; unavailable support is not replaced with invented numbers.

## Correction: Coz's own technique does not transfer to this workload; the substitute is simpler, not a translation

Coz's virtual-speedup mechanism works by inserting delays into *other concurrent
threads* relative to a chosen "progress point," which only produces a meaningful causal
estimate when there is genuine concurrency whose relative progress can be perturbed
(Coz's own evaluated workloads — PARSEC, Apache, SQLite — are all multi-threaded
server/parallel programs). This VM is explicitly single-threaded (round three's
standing rejection of concurrent GC), and every V8v7 suite runs as one deterministic,
single-threaded pass from start to end. There is no second thread to delay, so "a
controlled delay-balancing harness" that tries to approximate Coz's mechanism for this
workload is solving a harder problem than necessary and risks importing assumptions
(relative thread progress, sampling-based delay injection) that don't apply to a
single-threaded batch program.

The correct substitute for a single-threaded batch workload is simpler than a Coz
translation, not a weaker approximation of one: **direct region ablation**. To estimate
region `R`'s true upper-bound contribution to wall-clock time, replace `R` with a stub
that skips its actual work and substitutes a plausible precomputed/dummy result (breaking
correctness deliberately, for this measurement only, never for production code), and
measure the wall-clock delta directly. This gives an *exact* answer for "how much would
removing `R` entirely help," which is a strictly cleaner signal than Coz's *statistical*
virtual-speedup estimate — Coz exists because directly ablating code in a live
concurrent system is impractical without breaking correctness in ways that also perturb
other threads' behavior; neither obstacle applies to a single-threaded, single-pass
batch program, where ablation-then-measure is both feasible and more precise than a
delay-injection approximation of the same question.

Concrete steps for the ablation harness: build a stub-injection mode (behind a build
flag or environment variable, never shipped in the release binary measured for [[15]])
that lets a named region's real implementation be swapped for a fixed-cost/no-op stub;
measure wall-clock time with and without the stub for each candidate region under the
same alternating-sample discipline [[00]] already requires; record the result as an
exact upper bound on that region's addressable cost, not a statistical estimate — this
replaces the "controlled delay-balancing harness" language above with a concrete,
buildable mechanism suited to this project's actual (single-threaded, batch) execution
model rather than an approximation of a technique designed for a different one.

## Calibration log: track predicted-vs-actual, not just predicted-then-measured-once

A latency-weighted-dependency-chain prediction (via `llvm-mca` or the ablation harness
above) is only trustworthy if it is actually calibrated against this project's specific
target core, not generic published latency tables — memory-level parallelism, prefetcher
behavior, and this VM's own code layout can all shift real latencies away from a
textbook number. This task currently asks for a stated hypothesis before a candidate and
a measured result after, per instance, but nothing persists that pairing across tasks to
answer "is the prediction model actually getting more accurate, or systematically
wrong in a fixable way."

Add a standing, append-only calibration record (one row per gated candidate: predicted
critical-path delta, predicted mechanism, measured exact-gate result, agreement or
disagreement) alongside the existing per-task write-ups. Two concrete uses this unlocks
that a one-off per-task note does not: (1) if the model is systematically over- or
under-predicting a specific operation category (e.g. always overestimating the cost of
guard branches specifically), that is visible as a pattern across rows and fixable by
adjusting that category's weight, not by re-deriving the whole model; (2) it gives
[[301]]'s review cadence a concrete, quantitative trigger — a rolling prediction-accuracy
rate dropping below a stated threshold is itself a signal to recalibrate before trusting
the gate on the next candidate, the same way [[301]]'s existing "three same-tier tasks
without expected movement" trigger works, but for the predictive model specifically
rather than the tier ordering.

Acceptance: every gated candidate from this point forward adds one row to the
calibration record; the record is reviewed at the same cadence as [[301]]'s own triggers;
at least one systematic miscalibration (if any exists) is identified and its weight
adjusted, with the adjustment's own effect on subsequent prediction accuracy tracked
going forward rather than assumed to have fixed the problem.
