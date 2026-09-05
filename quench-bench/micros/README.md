# Diagnostic JavaScript microbenchmarks

This corpus exposes weaknesses in a tested VM without requiring any particular
architecture. A result may reveal sensitivity to calls, local count, changing
types, allocation lifetimes, or input size. It does not require a JIT, particular
collector, zero allocations, or a named optimization.

There are **24 experiment groups and 124 executable contrast variants**, plus
the **100 original numbered workloads**. Each group in `cases/` declares its
question, prerequisites, variants, and competing explanations. `experiments.json`
defines concrete sizes, separate development/qualification seeds, and the
measurement protocol. There is no fixed maximum case count.

## Quick start

Run commands from the repository root. The runner requires Node; Bun is the
default comparator. It does not build, edit, or optimize any tested engine.

```sh
# Discover questions and variants.
node quench-bench/micros/run.mjs list

# Validate the corpus using installed Node and Bun, without requiring Quench.
node quench-bench/micros/run.mjs smoke --engine node

# Use an existing Quench binary for semantic validation.
node quench-bench/micros/run.mjs smoke calls --engine target/production/quench-node

# Include the preserved original workloads, or select one by its old ID.
node quench-bench/micros/run.mjs smoke --engine node --include-legacy
node quench-bench/micros/run.mjs smoke 041 --engine node

# Exercise reserved seeds and equivalent wrapped sources at all sizes.
node quench-bench/micros/run.mjs smoke --engine node --reserved --size all
```

`--engine` chooses the candidate, `--bun` the comparator, and `--oracle` the
semantic oracle. All accept executable paths or names on PATH. Defaults are
`target/production/quench-node`, `bun`, and the Node running the harness.

The same generated JavaScript payload is executed by each engine. The candidate
must support plain script execution; unsupported syntax/APIs are reported as
failures, never silently skipped. Async cases must actually complete.

## Measure a weakness

```sh
# Compare inline/direct/changing/receiver/bound/multi-argument calls.
node quench-bench/micros/run.mjs measure calls --engine target/production/quench-node

# Measure every size together so the report includes scaling points.
node quench-bench/micros/run.mjs measure locals --size all --engine node

# Focus on one contrast; controls absent from the report remain missing evidence.
node quench-bench/micros/run.mjs measure arrays --variant grow --size large --engine node

# Short development run; this cannot award qualification.
node quench-bench/micros/run.mjs measure calls --engine node --pairs 2 \
  --warmup-ms 100 --window-ms 50 --out target/micros/my-calls.json
```

Development defaults are three process pairs, 100 ms warmup, and three windows
of at least 50 ms. Run on an idle machine for interpretable measurements. The
runner uses balanced alternating engine order and independent processes.

Setup is outside the measured window. Operations whose question concerns
allocation or construction intentionally perform it inside the workload.
Compare time **per invocation within a scenario**. `work_per_call` is the
declared input size, not a universal count of VM operations. Variants may do
different required work; their timing difference alone is not a defect.

Every measured process is checked against the oracle. Explicit equivalence
groups also verify that related variants compute the same result. The result
encoding preserves undefined, negative zero, NaN, infinities, BigInt, and lone
surrogates. Timing windows are nested samples, not independent repetitions.

## Inspect results and choose the next experiment

Each command prints the artifact path. By default it creates a unique directory
under `target/micros/`; `--out` must name a new `.json` file. Existing attempts
are never overwritten. A Markdown report is written beside the JSON.

```sh
node quench-bench/micros/run.mjs next --report target/micros/my-calls.json
```

The JSON contains raw process results, exact engine paths/hashes/versions,
source identity, scenario inputs, timing windows, RSS, ratios, confidence
bounds, controlled contrasts, and scaling points. Reports checkpoint after each
completed scenario. `--limit N` deliberately produces an incomplete report.

Interpret the fields separately:

- `correctness`: semantic agreement, not a performance score.
- `timing` / `memory`: `pass`, `fail`, `inconclusive`, or `invalid` against the
  declared target. A lower candidate/comparator ratio is better.
- `contrasts`: descriptive within-engine comparison against the first variant;
  semantic equivalence is explicitly labeled.
- `next`: missing controls or follow-up evidence. It never claims a root cause
  from timing alone or mandates an implementation. Commands are argument arrays
  preserving the tested engine paths; diagnostic commands need an existing
  `--trace-engine` to obtain internal observations.
- `qualification`: only a complete, frozen, full-protocol run can pass.

Smoke failures exit 1. Measurement target losses are recorded findings; a
semantic failure exits 1. Invalid CLI/configuration exits 2. An unsuccessful or
incomplete qualification exits 1. A corpus may be delivered successfully while
the tested engine fails many cases.

## Request optional instrumentation

Supply an **already existing** Quench trace-enabled binary. The harness will
not change runtime code or build a new binary.

```sh
node quench-bench/micros/run.mjs diagnose calls --variant direct \
  --trace-engine target-exec-trace/bench-throughput/quench-node --instrument counters

node quench-bench/micros/run.mjs diagnose objects --variant read \
  --trace-engine target-exec-trace/bench-throughput/quench-node --instrument sites

# Focus existing site output by CODE:PC or the build-qualified ID in the report.
node quench-bench/micros/run.mjs diagnose objects --variant read \
  --trace-engine target-exec-trace/bench-throughput/quench-node \
  --instrument sites --site 2:3
```

The adapter preserves the raw `QUENCH_EXEC_TRACE` snapshot and publishes its
capabilities and limitations. Current engine top-site lists are partial;
an absent site is **unknown**, not zero. Source identifiers are not claimed to
be file/line spans. Counts are not CPU-time measurements. Attribution includes
setup and validation, and instrumentation can perturb behavior.

`--instrument events` is accepted but reports **unavailable**: the current
snapshot interface does not expose chronological events. Site filtering selects
already-collected observations; it cannot recover sites the engine omitted.
No invented events, source lines, allocation bytes, or root-cause claims are
generated. Instrumentation availability never affects performance verdicts.

Use a separate uninstrumented `measure` run for performance. A diagnostic build
is identified independently and is not assumed to match the current source
revision merely because its executable path looks familiar.

## Memory lifecycle measurements

```sh
node quench-bench/micros/run.mjs measure lifetime --variant cycles --size large \
  --engine node --lifecycle --epoch-calls 100 --timeout-ms 600000
```

Peak RSS uses separate fixed-work processes, not timed windows in which a faster
engine performs more allocations. Lifecycle runs use 120 equal-work epochs and
three independent candidate processes. Calls per epoch are calibrated once from
the candidate's measured invocation time to target at least 250 ms, with a floor
of 100 calls and cap of 1,000,000. That fixed count is recorded and reused for all
epochs and replicates; work is never adapted inside a lifecycle run.
The final 60 epochs assess plateau and
reuse with a growth allowance of `max(8 MiB, 5% of late-run median RSS)`.

Current RSS is sampled externally every 100 ms. Phase attribution uses received
markers and is approximate, not a synchronous heap snapshot. Too few late-epoch
observations yield `inconclusive`; increase fixed `--epoch-calls` in development.
The runner does not force GC or require RSS to return to startup. Plateau is
finite evidence, not proof of leak freedom. RSS is never substituted with
physical footprint. The tested process itself is measured, not an arbitrary
descendant process tree.

## Full qualification

```sh
node quench-bench/micros/run.mjs qualify --edition 1 --idle-confirmed \
  --engine target/production/quench-node --out target/micros/qualification-1.json
```

This is a long run: 744 reserved new scenarios plus 100 legacy workloads. Each
scenario uses 30 throughput pairs and 20 fixed-work RSS pairs; applicable cases
also run lifecycle checks. Allow many hours, potentially days. Development
commands and limited runs never qualify an engine.

Qualification is currently ARM64 macOS only. `--idle-confirmed` is an operator
attestation: stop builds and unrelated benchmarks yourself. The runner does not
claim to detect all external interference or stop other agents.

The upper one-sided 95% ratio bound must be at most 0.95 for execution time and
1.05 for RSS. Bounds use 10,000 deterministic bootstrap resamples of independent
process pairs. Startup is separate. Every required result must pass; missing
metrics, timeouts, semantic disagreement, and uncertainty cannot be averaged
away. Legacy cases retain their original fixed inputs and have no holdout claim.

`editions/1.json` freezes source, metadata, and runner hashes. Modified corpus or
protocol files invalidate that edition. Use `identity` to inspect the current
hash map; establish a new edition rather than replacing published history.
Changing engine binaries during a run also invalidates qualification.

## Extend the corpus

Add a plain `.js` file under `cases/` calling `registerMicro` once. It declares
`id`, `question`, `requires`, `axes`, `observations`, `explanations`, `setup`, and
two or more named `variants`. Optional `equivalent` groups express identical
results; `check` validates effects; `release` drops workload-retained state;
`memory: true` enables lifecycle qualification; `async: true` requires awaited
completion. Functions receive the setup state. Each invocation must produce a
deterministic result for the scenario.

Add its path to `experiments.json`. Include controls, meaningful scaling, and
changing conditions. Do not require a VM mechanism or use fixture recognition
in production. Keep outputs small and avoid printing from workloads. Cases may
allocate and retain data, but must bound the intended live set explicitly.

```sh
node --test quench-bench/micros/tests/harness.test.mjs
node quench-bench/micros/run.mjs smoke <new-id> --engine node
node quench-bench/micros/run.mjs smoke <new-id> --engine node --reserved --size all
```

See `DESIGN.md` for decisions and terminology. The old `verify.mjs` and
`harness.cjs` remain available for historical numbered-corpus workflows; use
`run.mjs` for the extensible diagnostic corpus and qualification rules.

See [VALIDATION.md](VALIDATION.md) for the delivered corpus/harness checks and
their limits. Progress lines report semantic correctness, not performance wins.
