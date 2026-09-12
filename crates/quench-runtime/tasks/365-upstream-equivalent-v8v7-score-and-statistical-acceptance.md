# 365 — Upstream-equivalent V8v7 score and statistical acceptance lane

Status: complete

Add a final acceptance mode that preserves the upstream V8v7 execution and score semantics.
Load all eight suites in the upstream fixed order in one VM process, preserve the discarded
warmup and at least one-second measurement window with at least 32 iterations, retain the
unrounded timing/reference ratio for every suite, and compute the final geometric mean from
those raw ratios. Keep the existing split-suite mode for attribution and short screens only.

Run baseline and candidate as independently launched processes in randomized A/B or B/A
order for at least nine pairs. Report every raw observation, paired log ratio, median,
confidence interval, aggregate geometric mean, per-suite regression, binary SHA, Rust
toolchain, OS/CPU, and available thermal/power state. Never randomize suite order inside the
upstream-shaped process. A confidence interval including no change is inconclusive rather
than accepted.

Derive the score records directly from one machine-readable benchmark result stream; do not
parse rounded human display strings. Name all timing, run-count, repetition, confidence and
regression limits. Validate the driver against an unmodified upstream shell-capable engine
and add tests showing that raw-ratio aggregation differs from aggregation after formatting.

Acceptance: exact mode matches the upstream driver on deterministic fixture timings; one
current deegen run validates every suite in one process; the existing 2408.43 split-suite
smoke is relabeled as a development metric; `scripts/perf-cycle.sh accept` uses exact mode
and refuses fewer than the named minimum independent pairs.

Primary sources: upstream V8v7 `base.js`
<https://chromium.googlesource.com/v8/v8.git/+/dd3f1ecf719afd21b4c695c776b4da2fb494ef92/benchmarks/base.js>,
`run.js`
<https://chromium.googlesource.com/v8/v8.git/+/dd3f1ecf719afd21b4c695c776b4da2fb494ef92/benchmarks/run.js>,
Google Benchmark random interleaving
<https://github.com/google/benchmark/blob/main/docs/random_interleaving.md>, and Kalibera/Jones
<https://kar.kent.ac.uk/33611/45/p63-kaliber.pdf>.

## Implementation and evidence

`scripts/run-v8v7.sh` now has three explicit modes. The existing `split` mode remains the
short attribution tool. `same-process` keeps adjustable development timings while loading
all suites in canonical order. `exact` requires `all`, copies `base.js` unchanged, ignores
development timing variables, and preserves the upstream one-process warmup/measurement
sequence. The upstream callbacks and all benchmark execution finish before an audit trailer
prints version, success, ordered suite references/timings/raw ratios/raw scores, and the raw
aggregate ratio/score.

`scripts/v8v7-raw-results.py` fails closed on version/success, missing, duplicate or reordered
suites, malformed/non-finite/non-positive measurements, suite reference/timing/ratio
disagreement, rounded score input, and aggregate disagreement. It emits the one normalized
`v8v7-exact-v1` JSON fact consumed by `scripts/perf-cycle.sh`.

`scripts/perf-cycle.sh exact` records one or more complete process runs. `compare-exact`
performs the incremental exact gate. `accept` and `goal` default to the 10000 objective.
Baseline/candidate processes use recorded randomized AB/BA order and require at least nine
pairs. `scripts/v8v7-paired-stats.py` validates the paired records and reports geometric
means, medians, every process-pair log ratio, deterministic paired-bootstrap confidence
intervals, suite regression floors, and the goal gate. A confidence interval containing
zero fails an enforced comparison.

The deterministic exact-driver fixture uses the unmodified upstream `base.js`, overrides
only its clock and benchmark bodies, and proves that exact mode ignores development timing
variables: every benchmark performs four discarded warmup calls plus 32 measured calls over
eight timing chunks. It also proves that an exact raw score of 100.49 is not replaced by the
displayed rounded score 100, checks all eight suites in one process, and rejects single-suite
exact mode with the usage-error status. Raw-result and paired-statistics unit tests cover
their pure validation/math kernels.

An exact current-runtime run is archived in `reports/task365-current-exact.jsonl` with raw
output and metadata. Binary
`a1bcd95baf8b922ee59fd74cfcd7cc71ddd87fb3e10deac185c33daba4b0e6d1`
scores **2390.00712577743** in one process: Richards 964.09, DeltaBlue 937.40, Crypto
1846.40, RayTrace 1925.02, Earley-Boyer 3318.16, RegExp 3567.38, Splay 3963.64, and
Navier-Stokes 7063.91. This replaces 2408.43 as the authoritative current goal score; the
latter remains a split-suite development smoke only.
