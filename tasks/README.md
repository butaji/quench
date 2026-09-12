# Executing the task queue

[index.json](index.json) is the only authority for status, dependencies and lane
order. Start at `next_task`, then advance through `lanes.critical_path` only when
its prerequisites are complete. `critical_path` is an ordered dependency spine:
each item may depend only on an earlier critical item (it may intentionally skip
one). Side-lane items may depend on earlier items in their own lane, but no
side-lane item may become a prerequisite of the critical path.
Work in `profiled` and `host` is independent side work; it must not be a
dependency of the critical path and does not block it. `deferred` items require
their stated activation condition.

`pending` means not started, `in_progress` is the single active implementation
item (normally `next_task`), and `deferred` requires its activation condition.
Do not mark work complete in place: remove the completed item and repair the
queue as part of the completion check.

Each task defines outcome, current gap, scope/sequence, verification, definition
of done and performance evidence. Dependencies are in the index, not duplicated
in Markdown. Follow [repository rules](../AGENTS.md) and
[the performance protocol](../docs/performance-lanes.md).

Each representation or state machine has one owning task. Later tasks consume
that authority; they may extend its consumers but must not introduce a second
opcode, layout, activation, ownership or evidence contract.

## One roadmap model

The implementation is a single derived-data pipeline:

`OXC -> canonical Op/IR -> CFG and liveness facts -> value/backing ownership
-> completed cross-tier transitions -> property/call caches (093) ->
lifetime and executable leases (094) -> effect-aware optimization (095) ->
M4 production policy (097)`.

These labels describe ownership and consumption only; `tasks/index.json` is the
sole source for the actual order, dependencies and status. The arrows are
enforced by its critical-path dependencies as well as by queue order; a task
cannot be advanced merely because it is independently useful.
They describe consumers, not duplicate representations. The baseline CFG
authority owns control shape and baseline retirement; the value/backing authority owns JS value and backing
identity; the completed cross-tier authority is the consumer that combines those facts at calls,
exceptions, suspension or OSR. Task 095 may derive optimization graphs from
the same facts, but cannot replace either authority.

Evidence has one equally simple separation: execution-profile JSON records the
verified hot-IR lowering contract; runtime tests prove semantics and native
entry; V8-v7/Bun/JSC runs measure production speed. A result in one column is
never silently promoted to a claim in another column.

The ownership map is deliberately small:

| Authority | Owning task | Later consumers |
| --- | --- | --- |
| Canonical operations and effects | Existing operation declarations | Every execution and lowering view |
| Static function layout and activation | Existing `CodeStore`/`FunctionLayout` foundation | CFG, calls, roots and exits |
| Baseline CFG, joins and resident loops | Completed baseline authority | Ownership, cross-tier and optimizing execution |
| JS value and backing identity | Completed value/backing authority | Cross-tier, inline caches and native views |
| Cross-tier transition protocol | Completed cross-tier authority | Caches, optimization and host reentry |
| Property/call cache state | 093 | Optimizing guards and invalidation |
| Reclamation and executable leases | 094 | Every owner and safepoint |
| Derived optimization analyses | 095 | Qualified production policy |
| M4 production tier policy | 097 | Release decisions only; evidence remains in the performance protocol |

If a proposal spans two rows, extend the earlier authority and add a consumer
step; do not create a parallel representation. The single active item is
currently the queue's `next_task`; no later task is considered started merely
because its design is discussed.

The first two authority rows in the table are completed foundations, not the
first two queue items. Their contracts are immutable inputs to the remaining
work: no task may introduce a second
operation vocabulary, function-layout record or activation representation.

## Evidence boundaries

The shared evidence vocabulary is summarized in
[`docs/README.md`](../docs/README.md#shared-vocabulary); this section applies it
to task completion and queue decisions.

The normative schema, the meaning of “342 green” and the current architecture
mode status live in the [execution-profile contract](../docs/execution-contract-tests.md).
The queue consumes that contract: JSON is the best *verified canonical target
for the current lowering policy*, not a globally optimal IR or machine-code
claim. Stencil bytes, ABI, entry counts and native execution belong to focused
runtime tests. V8-v7/Bun/JSC scores are separate production evidence and require
a complete valid run under the [performance protocol](../docs/performance-lanes.md).

The queue therefore has four deliberate non-goals: execution-profile JSON is not
a proof of globally optimal machine code; 342 green is not a speed or conformance claim; a
semantic fallback is not silently promoted to native code; and host cleanup,
profiling and deferred proposals do not gate the VM critical path. Copy-and-patch
stencils are the physical mechanism selected for the JIT, while canonical
operations remain the single semantic authority.

## Completion

- Implement the complete specified capability through production consumers.
- Remove superseded state and mappings after all consumers migrate.
- Run focused controls, all 342 execution contracts for runtime changes, and the
  full runtime suite. Record named existing failures and prohibit new failures.
- Run relevant Node/Test262/Wasm checks when shared behavior changes; compare Node
  host behavior with the local oracle. A test command with no matching tests is
  not evidence.
- Run `node tools/check-task-coherence.mjs` after queue or documentation
  edits; task headings, lanes, dependencies, next-task order and local links
  must remain consistent.
- For performance work, freeze matched binaries and run the full V8-v7 comparison,
  with held-out semantic controls and startup/compile/code/RSS tradeoffs.
- Store raw evidence under a new ignored artifact path with build/source identity.
  A functional prerequisite can close without a speed claim; a performance task
  cannot close on an unmeasured hypothesis.
- Remove completed task files/items, repair dependency references and lane
  membership, and advance `next_task`. Committed task history remains in Git.

A discovered defect becomes a concrete prerequisite with a reproduction,
implementation scope and definition of done. Do not close implementation tasks
with research notes, zero native-entry counts, renamed IR, partial benchmark
scores, or an ever-growing progress diary.

Task scope is finite; the VM has no arbitrary optimization ceiling. Larger
regions, richer optimization, register allocation and SIMD are allowed when
their assumptions, cost and fallback are explicit. Resource policies protect
execution and compilation; revising them requires correctness and measurement.

The active critical path is intentionally one bounded authority transition per
task: caches (093), lifetime (094), optimizing compilation (095)
and M4 qualification (097). Earlier opcode, activation and straight-line
foundations are prerequisites already in the implementation and are not task
queue entries. Evidence and artifact lifecycle
are completion checks for those transitions, not extra queue items. Do not add a
parallel task for a representation already owned by one of these authorities.

When two pending proposals share one state machine, fold them into its owning
task instead of keeping a second queue item; Git history preserves the retired
task narrative.
