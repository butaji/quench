# Goal

Identify and correct the highest-impact, evidence-supported architectural
weaknesses in Quench's general-purpose JavaScript VM, following `AGENTS.md` and
`$lisp-mindset`.

Optimize for correct semantics, bounded execution work and memory retention,
and measured runtime and RSS improvements. Native execution counts alone are
not success criteria: compilation, code size, startup and retained memory belong
in the tradeoff.

## Establish evidence

Run every enumerated microbenchmark and `v8_v7` workload. Record the exact
workload inventory, repository revision and working-tree changes, build
configuration, toolchain, hardware, commands, outcomes, and measurement
variability. Failures and unsupported workloads remain visible and are
investigated.

Collect instrumented profiles and DWARF-backed attribution, supplemented by
allocation measurements, compiler output, and machine-code inspection where
needed. Use separate performance runs to validate improvements and account for
diagnostic overhead.

Map the relevant subsystems, semantic ownership, data representations, execution
paths, and benchmark coverage. Distinguish measured observations, causal
hypotheses, and unresolved questions. Benchmarks must be supplemented by semantic
and architectural review.

## Choose corrections

Investigate root causes rather than ranking changes solely by hot functions.
When evidence is missing, define and perform the experiment needed to resolve
the decision.

Research primary sources, including Deegen, to challenge assumptions. Evaluate
stencil approaches at clearly named layers alongside alternatives. Record why
each proposed approach fits Quench's semantics and constraints.

Prioritize corrections by correctness, expected benefit, evidence strength,
complexity, and implementation risk. Each selected correction must state its
hypothesis, affected invariants, alternatives, validation plan, and acceptance
criteria before implementation.

## Implement and validate

Implement corrections in Rust. Represent semantic facts once and generate
repetitive consequences where useful. Use macros when they eliminate duplicated
knowledge while preserving explicit exceptional behavior and understandable
generated code.

Preserve complete ordinary semantics. Every fast path must have justified guards
and a correct fallback. Validate observable JavaScript behavior and check Node
compatibility against the local Node oracle and upstream source where applicable.

Validate each correction incrementally, then rerun the full benchmark inventory.
Report performance distributions and regressions, along with relevant memory,
generated-code, static-data, cache, and native-code costs.

Production code must never recognize benchmark fixtures or contain
benchmark-specific behavior. All improvements must apply to general-purpose
JavaScript execution.

## Make improvements durable

Adopt the [execution-contract test design](docs/execution-contract-tests.md):
ordinary JavaScript fixtures, Rust declarations, and one canonical vocabulary of
measured facts. The DSL is a proposed implementation contract, not an existing
API or evidence that its example budgets currently pass.

For each selected correction, add the smallest meaningful regression contract:
semantic outcomes plus bounded work or retained resources, with input-size or
input-diversity cases where growth is the risk. Pair guarded optimizations with
fact-breaking cases that exercise the correct fallback. Reserve exact execution
tier quotas for targeted mechanism tests; eliminated work is a valid improvement.

Keep three complementary forms of evidence: deterministic execution contracts,
memory ownership/lifecycle contracts, and uninstrumented process benchmarks.
Allocation counts and tracked live bytes do not establish RSS or elapsed-time
improvements. Record cold and warmed behavior and explicit cleanup boundaries.

Before trusting counter contracts, establish that measurement cannot influence
quickening, admission, object lifetime or execution decisions. Missing, truncated
or unavailable observations cannot satisfy zero-cost assertions. Derive names,
units, aggregation and validation from the same Rust declarations; keep report
formatting and test expectations outside production execution.

Implement this incrementally alongside corrections in the canonical task queue.
Preserve the existing stencil infrastructure gate before micros and benchmark-led
tuning; the DSL does not replace that gate or require an unrelated telemetry
rewrite. Acceptance requires executable harness checks and representative JS
contracts, followed by reproducible performance evidence when that gate permits.

## Track and finish

Keep the architecture findings and decisions in `docs/`, and maintain one
canonical task register in `tasks/`, linking to evidence instead of duplicating
status.

After the initial investigation, define a bounded correction batch with explicit
acceptance criteria. Complete that batch, publish reproducible before/after
evidence, and document remaining findings and unknowns. Expand the batch only
with a recorded reason.
