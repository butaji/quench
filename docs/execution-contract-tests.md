# Execution contracts for JavaScript fixtures

Status: partial implementation. The reusable contract DSL below remains proposed;
the bounded planner and physical-layout contracts already have executable unit and
normal-driver coverage. Gate status belongs in the [task register](../tasks/index.json).

## Current bounded optimizer contract

Quench adopts the useful local ideas from general compilers and assemblers without
embedding Cranelift or DynASM. `stencil_value_graph.rs` builds one disposable,
eight-value graph over canonical residual instructions. It performs exact constant
propagation/folding, local value numbering, dead-pure-node marking and costed fusion;
effectful, coercive, live-out or unknown inputs reject to ordinary execution.
`stencil_region_layout.rs` keeps operation/exit points symbolic until it derives
fragment labels and checked fixups, then publishes transactionally through the
existing arena. The selected runtime composition path uses this representation.

Current deterministic evidence at `4c7851cda7` is planner 26/26, fused normal-driver
11/11, property integration 5/5, and region layout 11/11. These tests cover folded
constants, eliminated dead loads/moves, repeated-value reuse, ordered add trees,
guard-breaking fallback, semantic CFG edges and failed composition without partial
output. They do not establish a general SSA optimizer, global register allocator,
arbitrary block composition, or a runtime assembler. Those larger mechanisms remain
deliberately out of scope or open under task 075.

## Purpose and boundaries

Make measured improvements durable with small tests of semantic outcomes,
avoidable execution work, resource bounds and ownership. A passing contract
does not prove lower runtime or RSS. Validate those separately using the
[performance lanes](performance-lanes.md), after the existing infrastructure gate.

Follow [repository rules](../AGENTS.md) and the Lisp mindset: facts once, derived
views, explicit effects, shallow composition. Use Rust for the runner and macros;
JavaScript is fixture input parsed by OXC. Keep language tests in quench-runtime
and Node host fixtures/adapters in quench-node. Do not build a competing syntax
tree, semantic interpreter, ownership registry or general telemetry framework.

## Authoring surface

Start with a Rust macro expanding to ordinary Cargo tests and typed test data:

```rust
execution_test! {
    numeric_loop {
        js: r#"
            function run(n) {
                let sum = 0;
                for (let i = 0; i < n; i++) sum += i;
                return sum;
            }
        "#,
        cases: [10, 1_000, 100_000],
        measure: run(case),
        expect: number(case * (case - 1) / 2),
        counters: {
            allocations.environment == 0,
            allocations.object == 0,
            deopts == 0,
        },
    }
}
```

The example measures the entire invocation: an environment needed for function
entry would also count. Establish actual behavior before adopting a zero budget;
use a justified constant bound if required. Do not silently exclude entry work
to make a loop contract pass. Arguments are runtime inputs to ordinary execution,
not injected compiler facts.

Allow `js: include_str!("fixtures/numeric_loop.js")` through the same source field.
Initially support named global callable entries with primitive arguments and
ordinary return/throw expectations. Expected expressions above execute in Rust
outside the interval. They are not another expression language for the runner.
Keep JS numeric distinctions such as NaN and signed zero in typed expectations;
do not compare results through lossy JSON serialization. Complex semantic checks
can use existing assertion helpers after measurement; checking code must not
execute inside the counter interval.

Counter constraints accept `==`, `<=`, and `>=`; omitted metrics are unconstrained.
Unknown metrics, incompatible units and duplicate contradictory declarations are
errors. Empty cases or no executed observations cannot count as a passing suite.
An optional literal `warmup: 10` performs ten unmeasured invocations with the same
arguments before the single measured invocation. Default warmup is zero. Fixtures
with mutable state must explicitly expect the post-warmup state.

Exact tier assertions remain useful for targeted mechanism tests:

```rust
counters: {
    native.entries == 1,
    fast.instructions == 0,
    dynamic.instructions == 0,
    deopts == 0,
}
```

These describe different units, not three mutually exclusive kinds of JS call.
Native means entry into generated machine code; Fast counts actual compact
handler execution and Dynamic counts actual general-handler execution. A native
entry can retire many iterations or deopt into handlers. Count each actual event;
do not reconstruct executed instructions from source or inferred loop length.
Separate successful native completion from entry when completion matters.
Define host calls and JS calls independently if a concrete correction needs them.
Do not impose these quotas on general efficiency contracts: eliminating work
entirely may legitimately reduce native entries to zero.

## Canonical data and evaluation

Extend the existing declarations in `execution_trace.rs` and the
[observability design](observability-architecture.md), instead of maintaining a
test-only metric catalogue. Declare each metric's ID, wire name, unit, measured
population, supported scope and aggregation once in Rust data/macros. Derive
typed accessors, indices, report names and validation from those declarations.

The minimal conceptual data model is:

| Data | Contents |
| --- | --- |
| Contract | Source, cases, invocation, warmup, outcome and constraints |
| Metric definition | Identity, unit, population, scope and aggregation |
| Constraint | Metric ID, comparison and typed bound |
| Observation | Metric ID, interval, availability and measured value |
| Violation | Constraint, actual observation and optional site evidence |

Use enums for comparison, aggregation and observation availability. A pure
evaluator maps constraints and observations to violations; the execution adapter
owns VM effects and the reporter owns formatting. The macro constructs this data
and tests; it must not contain another runner implementation. Avoid a plugin
registry or generalized query language until a concrete use earns it.

Counters are folds over observed events, not a requirement to store event logs.
Use dense counters for fixed kinds. Detailed site evidence is optional and bounded;
its truncation must remain visible. Exact totals must stay complete independently
of site-map capacity. Do not retain executable or object owners for diagnostics.

## Measurement state machine

Use an explicit lifecycle:

`Create VM -> Load -> Resolve/prepare -> Warm up -> Begin -> Invoke -> End -> Check -> Drop`

- Each case owns a fresh VM. Warmup deliberately preserves its execution and heap
  state; opening measurement resets neither caches nor semantic state.
- Loading, argument conversion, callable lookup, warmup, result checking and
  formatting are outside the measured interval. Include callee activation,
  transitive synchronous calls, helpers and fallback work during invocation.
- Close the interval on normal return and JS throw using a scoped guard. Internal
  runtime faults are harness failures, not expected JS exceptions or deopts.
- Initial support is synchronous. Promise settlement, microtask draining, worker
  attribution and host event-loop completion need explicit future adapters;
  never imply that a synchronous return measures asynchronous completion.
- Reject nested intervals initially. Scope ownership and availability explicitly;
  process-global counters cannot masquerade as per-VM observations under parallel
  Cargo tests. Use isolation for metrics that require it.
- Missing capabilities produce an explicit unsupported result. Required CI
  configurations treat unsupported contracts as failures; optional configurations
  report them separately. Truncation or overflow fails affected exact assertions.
- Feature-disabled instrumentation is unavailable, not observed zero. A sparse
  missing event means zero only when the declared metric is active and collection
  for its population and interval is complete.

Use snapshot deltas only for monotonic cumulative counters. Live-byte gauges need
opening and closing readings; interval peaks require tracking within the interval,
not subtracting two lifetime high-water marks. Specify reset and overflow behavior
for every aggregation. Metric coverage must include all relevant execution tiers.

Tracing currently exposes `quickening_prefers_hot` to execution decisions. Audit
and separate adaptive runtime facts from diagnostic state before trusting these
contracts. Enabling, disabling or beginning measurement must not change policy.
Compare semantic results and decision evidence with observation enabled/disabled;
measure observer time and memory overhead separately without claiming zero cost.

## Memory and growth contracts

Add only metrics required by concrete regressions. Distinguish logical JS creation
from physical header/backing allocation, resize, final destruction, code mapping
and reserved capacity. Allocation count alone does not bound bytes. A live gauge
requires matching allocation/free populations and an opening state, not subtraction
of incompatible historical lifecycle counters.

Use existing owners and resource ledgers to test bytecode, native code, metadata,
cache capacity and retired-but-live bytes. State whose memory is charged, when it
is released and whether shared resources are counted once. Do not add a second
heap graph to answer test queries.

Lifecycle scenarios need explicit checkpoints beyond invocation: release fixture
roots/results, run the existing documented cleanup operation if applicable, then
drop the VM. Native leases and escaped callables remain legitimate owners until
released. Use ownership/drop tests where JS fixtures cannot expose this boundary;
do not force Rust ownership assertions into the JavaScript DSL. Do not assume that
function return or forced cleanup represents natural production reclamation.

Test growth by varying one independent dimension with fresh state per case:

- More loop iterations: no per-iteration environment allocation.
- More receiver shapes: cache storage stays within the declared policy capacity.
- More independently compiled functions: code/metadata remain charged to owners.
- Repeated create/run/drop cycles: resources return to a documented baseline after
  the final owner is released, allowing explicitly bounded shared caches.

Initially express these through cases and shared explicit bounds. Add cross-case
relations only when needed, with named dimensions and reproducible failure pairs.
Finite cases provide regression evidence, not an asymptotic complexity proof.

RSS, physical footprint and virtual memory are distinct process observations.
Measure startup, steady execution, churn and post-cleanup behavior in isolated
process benchmarks. Record absolute peaks and baselines, allocator/build/platform
configuration, repeated samples and variability. Lower tracked live bytes do not
guarantee that the allocator returns pages or that RSS falls immediately.

## Failure reporting

Report case/input, warmup, measured boundary, build/capabilities, semantic outcome,
and every violated constraint with expected/actual units. Add first guard failure
or source location only when recorded, using existing code identity/source maps.
No guessed reason or retained object graph merely to improve an error message.
Distinguish semantic failure, contract violation, unsupported measurement and
harness failure. Diagnostic output cannot turn a failing test into a pass.

## Incremental delivery and acceptance

1. Establish trustworthy measurement: canonical definitions, independent adaptive
   state, complete scoped totals, availability and isolation. Reuse existing hooks
   and follow skill caps: functions at most 40 lines and complexity 10, files at
   most 500 lines, with cohesive modules instead of arbitrary splitting.
2. Implement the typed evaluator and small macro with inline/file JS, primitive
   invocation, result/throw checks, comparisons, cases and explicit warmup.
3. Add representative contracts for loop work, frame sizing, bounded cache reuse
   and code-owner disposal. Keep lower-level ownership tests where appropriate.
   Pair each guarded optimization with invalidating inputs and correct fallback;
   include zero/one/many iterations, numeric edges and relevant observable effects.
4. Verify harness failure paths: unavailable metrics cannot pass zero assertions;
   warmup is excluded without clearing caches; throws close intervals; parallel
   cases do not contaminate totals; bounded diagnostics preserve exact totals;
   result checks are excluded; live/peak aggregation uses the correct boundary.
5. After the existing infrastructure gate, validate selected corrections with the
   unchanged benchmark inventory and uninstrumented runtime/RSS measurements.
   Record tradeoffs in startup, compilation, native bytes and retained memory.

Use explicit budgets justified by semantics, resource policy or measured baseline;
never bless current output through automatic snapshot updates. Native-required
tests must execute actual generated code on a supported configuration. Existing
benchmark profile contracts remain complementary; migrate shared metric knowledge
incrementally without introducing another JSON/string schema as the authority.

Completion means executable coverage and trustworthy observations for the bounded
correction batch, plus its required performance evidence. It does not mean every
possible metric, async host or platform is supported.
