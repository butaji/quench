# Architecture gap map: current research

This is a provisional synthesis, not a completed architecture audit. Source
`06e71d3f4` has completed all eight trace-disabled V8 attempts: seven valid score
outputs, DeltaBlue failure. Counter-enabled V8, all-size micros, production timing
and memory lanes are not yet complete. No full-suite score or performance win.
The latest user direction is documentation/tasks only: no messages to Codex and
no VM implementation changes by the researcher.

Evidence and reproduction: `target/architecture-survey-current/` contains the
verified source manifest, preserved executable identity, raw profiles, runner,
`profile-summary.json` and `partial-findings.md`. Earlier results under
`target/stencil-review-1788646448/` are separately identified historical evidence.

## Shared gaps, not one optimizer per benchmark

| Architectural area | Current evidence | Recommended direction | What remains unproved |
| --- | --- | --- | --- |
| Static activation metadata | Richards/Splay frame-width traversal appears in trace-off stacks; source recursively visits immutable fragments | Derive frame requirements once; share activation/exit/root contracts | Whole-run savings and best slot-storage strategy |
| Layout identity and queries | Cached interned layout coexists with visible-name shape scanning; property iteration decodes owned values | One immutable layout record with distinct derived views; key-first internal queries | Benefit versus metadata bytes; mutation and identity-domain migration coverage |
| Array/object storage ownership | Current Crypto classification isolates repeated existing sparse numeric writes; historical slice evidence isolates intrinsic destination construction | Stable identity with mutable backing; implicit ordinary attributes and precise invalidation | Remaining array lanes, retained bytes and broader migration cost |
| Collection graph and capture lifetime | EarleyBoyer trace-off cycle/environment traversal; historical lifecycle accounting limitations | Actual owning-edge model, shared root visitation, selective capture; evaluate collector alternatives after measuring | Whole-run GC share, edge multiplicity, collection frequency, pauses and best collector policy |
| Native region composition | NavierStokes dispatch/arithmetic/copy stacks under default policy; CFG/liveness/contracts already exist | Bounded costed composition, unboxed residency, effect-valid facts and exact exits | Actual opt-in native coverage and benefit; default-off events cannot prove rejection |
| Cold planning and admission | RegExp initial sample includes BaselinePlan::compile amid allocation/free | Measure cold plan construction and lifetime separately from matcher and steady-state execution | Setup versus recurring planning cost; no attribution to regexp engine from suite name |
| Instrumentation | Trace-enabled hashing versus different trace-off stacks; inconsistent historical lifecycle populations | Canonical generated IDs/units, bounded detail and explicit omissions | Fixed-work overhead, balanced live gauges and complete attribution |
| Semantic completion | DeltaBlue lacks score in both diagnostic modes | Preserve failure as missing performance coverage; shared ordinary semantics remain prerequisite | Cause and repair belong to implementation work, not this architecture inference |

These samples cover only initial three-second windows, include waiting threads
and startup, and overlap other work. They are not whole-run CPU percentages.
Allocation/free does not prove leakage or an inappropriate collector. A valid
score output does not establish complete JS conformance. Do not rank mechanisms
by raw counts from unequal, adaptive workloads.

## Fresh counter cross-check

The current Crypto trace classifies 28,186,835 of 28,536,661 named writes as
Number-to-Number updates of existing sparse own array properties. Only 57,369
create an absent physical hole. This replaces the older coarse stale/sparse
counter interpretation and directly motivated the canonical interior-mutable
sparse store in `d69e6971ea`. Matched Crypto score improves from 17.0 to 55.2;
the result is local throughput evidence, not a full-suite or RSS conclusion.
RayTrace still reports substantial owned reads and property-table clones, while
EarleyBoyer reports environment allocation/traversal. Those are separate open
ownership questions and must not be collapsed into the proven Crypto mechanism.
Lifecycle populations with drops exceeding allocations remain unsuitable for
live-heap subtraction until their accounting contract is repaired.

## Coherent design direction

Use the existing residual operations and Rust declarations as the semantic source.
Derive immutable code/layout structure before execution; retain dynamic facts only
where uncertainty remains. Give physical storage stable identity and explicit
mutation domains. Share activation, roots and exit reconstruction across ordinary
and optional native execution. Let bounded native plans consume these contracts,
not duplicate them. Keep instrumentation a disposable observation of the same
identities and events, not another runtime.

The decisions are detailed in [activations](activation-architecture.md),
[heap/layout identity](heap-identity-architecture.md),
[region composition](region-composition-architecture.md) and
[observability](observability-architecture.md). Their alternatives and constraints
matter: do not turn recommendations into blanket GC replacement, thousands of
stencils, another semantic IR, or unsafe borrows across callbacks.

Rust macros generate mechanical metadata and tests. rustc/LLVM compiles stencil
source offline; the runtime performs bounded selection, copying and patching.
No C stencil source or runtime optimizer. This is the lisp-mindset application:
one fact, derived views, explicit effects and composition over duplicate systems.

## Required challenge before accepting a solution

The later EarleyBoyer sample in `partial-findings.md` emphasizes dispatch and
tracing, unlike early trace-off collection stacks. Phase and instrumentation both
differ, so neither supports a whole-run GC-bound classification. Preserve this
counterexample when ranking collection work; require phase-specific corroboration.

For each row: finish the applicable corpus lanes, distinguish setup and steady
state, compare trace-off/counter observations and controlled fixed-work variants,
check held-out inputs, and inspect current consumers. Record competing explanations
and rejected hypotheses. Require matched before/after correctness, throughput,
RSS/lifetime, code/cache bytes and supported native-PC evidence where applicable.

Rank work from measured reusable benefit after prerequisites, not benchmark names.
The active implementation scope is [task073](../tasks/073.md), after the
[075 infrastructure gate](../tasks/075.md). Broad claims of optimality remain
unproven; unresolved and unreviewed areas must remain visible in the final audit.
