# Executing the queue

[index.json](index.json) owns task status, dependencies, lane order and stage
membership. There are exactly two stages; no other work blocks their completion.
The ordered `lanes` lists own lane membership; task records do not repeat it.
Stage membership remains separate because a lane can span both stages.

| Stage                       | Required result                                                                                                                              | Closure                                            |
| --------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| A — correctness and cleanup | One OXC/Wasm VM, no legacy or unused clutter, units only for internals or uncovered scenarios; all Node-adjudicated pinned Test262 cases, 100% Wasm, plus the framework-core Node profile and pinned Express/Koa/Fastify scenarios | [86](86.md), on the final cleaned production build |
| B — performance             | Best Score and lowest maximum RSS on **every** V8-v7 benchmark against QuickJS, Bun/JSC without JIT and Node/V8 without JIT                  | [61](61.md), recorded by [28](28.md)               |

## Stage A — single VM and correctness

- OXC JavaScript and Wasm execute on one production VM: one heap, Value, root
  set, opcode vocabulary, dispatch loop, activation stack and Engine entry.
- `quench-runtime` owns the single VM and OXC JavaScript syntax. Its public
  embedding operations serve `quench-node`, which owns Node APIs, module loading,
  I/O and event-loop policy. `quench-wasm` owns format decoding/validation and
  calls the shared runtime for execution. Necessary pure algorithms keep one
  surviving authority. CLI and suite runners compose these layers; extracting
  them into new crates adds no completion gate.

  [Task 27](27.md) completed the cutover and removed legacy execution. The
  final single-VM source audit and Task 86 qualification are recorded in
  [Stage A evidence](evidence/task86-stagea-final-2026-10-08.json).

- Remove legacy execution, migration adapters and audited unused clutter.
- Keep unit tests only for internals or scenarios absent from the pinned suites.
  A complicated regression may remain for its uncovered obligation; a regression
  label or multiple execution modes alone does not justify duplicate suite
  cases.
- Pass every Node-adjudicated pinned Test262 case, every pinned Wasm
  directive, every upstream fixture tagged
  `framework-core` in `parallel.txt`, and the pinned Express, Koa and Fastify
  scenarios. For direct Test262 conflicts, exact local Node runs with the
  pinned harness and the current ECMAScript algorithm arbitrate; preserve
  contradicted raw outcomes as waivers, never passes.
  The manifest owns fixture membership; `tests/frameworks` owns the package
  scenarios. The broad Node inventory is diagnostic only and adds no gate
  cases.

The three package scenarios define the Stage A framework target. Do not expand
the Node gate to unrelated upstream APIs or the broader proposals in
[88](88.md)/[89](89.md).

Stage A is closed. Task 86 records the final production qualification: all
67,124 Wasm directives pass, all 43 `framework-core` cases pass, and the
Express, Koa, and Fastify scenarios match Node exactly. Test262 passes 53,405
of 53,406 cases across all 116 stages. Its only raw failure is the unchanged
Stage 94 Annex B fixture contradicted by local Node; Task 20 records the
Node-arbitrated waiver, which remains outside the pass count. Stage 114 passes
all 1,482 cases. See the [final evidence](evidence/task86-stagea-final-2026-10-08.json)
and [Task 20 resolution](20.md#current-conflict-resolution).

The active critical-path work is:

- [62](62.md): add no-JIT reference engines to the V8-v7 runner.
- [61](61.md): lead the reference engines on all eight V8-v7 benchmarks.
- [28](28.md): record final per-benchmark Score and maximum RSS.

Stage A's final source, inventory, binary, and report provenance is captured by
Task 86. Stage B qualification begins with Task 62; measured optimization
work follows the evidence and correctness requirements below.

Task 24 is the sole initial full-inventory campaign and owns the same-build
reports for all raw Test262 outcomes plus the adjudicated pass count, Wasm and
the scoped Node gate. If it finds a non-waived failure,
reopen the responsible implementation task, fix and verify the affected scope,
then repeat Task 24.
Tasks 27 and 86 reuse those reports only when source, binaries, configuration
and inventories are unchanged.

The framework Node and shared-VM Wasm suite tasks are complete. Optional broad
Node embedding and JavaScript WebAssembly adapters do not block the selected
Stage A profile. Implement only a concrete failure found by the suite gates.

Task 24 depends on the three suite closures. Support tasks supply required fixes
and checks, including existing specialization fallbacks, roots, host re-entry
and exhaustion; completing their historical plans is not an extra gate. Phase
task lists identify ownership, not mandatory implementation checklists.

Task 86 closed Stage A and enabled Stage B qualification. Task 62 is active;
its reference-engine setup feeds Task 61's baseline and qualification, followed
by final closure in Task 28.

## Stage B — performance

Run every V8-v7 benchmark on Apple M4/macOS: Crypto, DeltaBlue,
EarleyBoyer, NavierStokes, RayTrace, RegExp, Richards and Splay. On **each
benchmark**, Quench must have both the best Score and the lowest maximum RSS
against QuickJS, Bun/JSC with JIT disabled and Node/V8 `--jitless`. An aggregate
win cannot hide a losing benchmark or memory result.

[Task 62](62.md) supplies matched reference-engine measurement; [61](61.md) uses
those same reference samples for its baseline, closes measured gaps and owns
qualification; [28](28.md) records final closure. Use task 61's sampling and
provenance contract. Optimization tasks are optional candidates selected by
measured gaps, not a mandatory implementation list. Prioritize applicable
measured v2 wins: compact residual facts (task 50), the physical phase
boundary (25), narrow local rules (33), and corpus PGO (84). Quench already
has direct site-indexed field caches; task 29's only v2-inspired candidate is
the measured own-property-only path. V2's uniform BTA (82) maps to task 57, with
task 68 supplying selected counts; extend Quench's existing analysis only for
domains that explain measured work. Census the current path first; prior
success does not replace a Quench paired experiment. Ordinary lexical reads
and writes already use local and capture slots. V2's task 28 measured dynamic
name lookup, so it can inform task 70 only if Quench's task 68 census finds the
same hot global-resolution work; it does not justify rebuilding direct local
addressing. Task 49 covers only activation facts Quench still rediscovers and
remains optional until a measured gap points there.
Keep method caches separate unless Quench's census identifies their cost: v2's
direct field-site index and own-property cache were retained, while its direct
megamorphic method index was removed after the expanded paired run. Remove
rejected experiments. No v2-parity, watermark, broad restructuring or
Wasm-speed requirement blocks either stage.

Stop when all eight benchmarks meet both targets and final correctness still
passes. Unselected optimization proposals do not create more work to complete.

Guide implementation with Ershov binding-time analysis, Futamura specialization
and the pinned `../v2` measured wins and rejections. Recheck assumptions against
Quench's current residual representation and admit changes only with affected
correctness checks and paired Quench Score/RSS evidence. Self research supplies
[object maps and customization](https://bibliography.selflanguage.org/implementation.html),
[site-local polymorphic caches](https://bibliography.selflanguage.org/pics.html)
and [type feedback](https://bibliography.selflanguage.org/type-feedback.html).
Use proven facts or guards with exact fallback; admit changes by paired
Score/RSS evidence. Production behavior never depends on benchmark identity,
source, scores, suite markers or the comparison engine. Quench remains
interpreter-only.

## Checks and evidence

During ordinary changes, run **only affected Test262 stages plus compare-runs**
against a matching inventory; include shared callers when changing a common
operation. Apply equivalent affected-scope checks to Node and Wasm, and record
why the selected scope covers the change. [Task 48](48.md) owns the ratchet.
Full runs belong only at gates 24/27 and stage transitions, including final
Stage A verification and Stage B qualification. Do not run full suites per edit.

Do not modify expectations, add skips, shrink inventories or count empty
discovery as a pass. New failures fail the check. Verify changed Node behavior
against local Node and relevant upstream source, including descriptors,
identity, ordering, errors, exit status and host effects.

Stage closure requires complete reports from the same final production source
and configuration, with exact inventories, suite revisions, binary, toolchain,
host and command provenance. Keep compact reviewable evidence summaries; large
raw artifacts belong under ignored `target/`. Reuse qualifying reports when the
source, binary, configuration and inventory are unchanged. A status update or
completion record alone does not require another full suite run. Reverify
affected scopes after changes; run the final full inventories once for the
resulting stage qualification.

## Queue discipline

Start at `next_task`; prerequisites must be done. Implement only obligations
needed for the two stated outcomes. The `deferred` lane is non-blocking and is
not a third stage; no active stage depends on it. Bring in a concrete part only
when correctness or a measured performance gap requires it.

Keep statuses truthful. Completed specifications retain short completion
records; Git history is the archive. Do not mark unused proposals done. Set
`next_task` to `null` when the critical path closes. Pre-cutover comparison uses
separate development binaries; production never selects a legacy fallback.

Apply [AGENTS.md](../AGENTS.md) and the Lisp mindset throughout: one authority
per fact, shared semantics, derived state, explicit effects and a reduction pass
before finishing. No mandatory line-count or complexity caps apply.

After the requested Quench work is complete and verified, commit and push the
changes. Deployment verification is outside the current scope.

The [task direction review](evidence/task-queue-direction-review.json) records
which specifications were corrected and which evidence remains unqualified. It
is a review snapshot; index.json remains the sole queue-status authority.
