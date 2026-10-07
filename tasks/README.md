# Executing the queue

[index.json](index.json) owns task status, dependencies, lane order and stage
membership. There are exactly two stages; no other work blocks their completion.
The ordered `lanes` lists own lane membership; task records do not repeat it.
Stage membership remains separate because a lane can span both stages.

| Stage                       | Required result                                                                                                                              | Closure                                            |
| --------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| A — correctness and cleanup | One OXC/Wasm VM, no legacy or unused clutter, units only for internals or uncovered scenarios; 100% Test262 and Wasm, plus the framework-core Node profile and pinned Express/Koa/Fastify scenarios | [86](86.md), on the final cleaned production build |
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

  The shared VM and legacy engine still have separate package and binary
  identities. [Task 27](27.md) folds the shared VM into the canonical runtime
  and removes transitional second-engine identifiers.

- Remove legacy execution, migration adapters and audited unused clutter.
- Keep unit tests only for internals or scenarios absent from the pinned suites.
  A complicated regression may remain for its uncovered obligation; a regression
  label or multiple execution modes alone does not justify duplicate suite
  cases.
- Pass 100% of pinned Test262 and Wasm directives, every upstream fixture tagged
  `framework-core` in `parallel.txt`, and the pinned Express, Koa and Fastify
  scenarios. The manifest owns fixture membership; `tests/frameworks` owns the
  package scenarios. The broad Node inventory is diagnostic only and adds no
  gate cases.

The three package scenarios define the Stage A framework target. Do not expand
the Node gate to unrelated upstream APIs or the broader proposals in
[88](88.md)/[89](89.md).

Stage A remains open: the pinned Test262 run still has the Stage 94 corpus
conflict, and production cutover, integrated qualification and final cleanup
remain outstanding. The current shared-VM Wasm and scoped Node reports are
partial evidence until task 24 and task 86 close their gates.

The remaining work is:

- [20](20.md): close audited Test262 behavior gaps with affected-stage checks.
- [19](19.md), [21](21.md) and [22](22.md): freeze and pass the manifest-owned
  framework profile and pinned package scenarios on the shared VM.
- [37](37.md), [40](40.md) and [23](23.md): finish shared Wasm lowering and
  integration and close known directive gaps.
- [24](24.md): verify integrated correctness; [27](27.md): switch all production
  consumers and delete legacy; [86](86.md): complete cleanup and verify the
  final build against all three inventories.

Prioritize failing suite cases, missing shared-VM execution paths and production
cutover blockers. Do not start discretionary architecture or new host
capabilities. Task 48's report/ratchet tooling is complete independently of the
100% suite gate. The existing parallel manifest and package lock are the fixed
Node target. Audit support tasks against required suite behavior and necessary
internal invariants. Close satisfied obligations with evidence and implement
only actual gaps. Historical plans, host rewrites, runner-speed targets, new
APIs and broad architecture proposals add no gates.
Performance campaigns are not required in A.

Task 24 is the sole initial full-inventory campaign and owns the same-build
100% reports for Test262, Wasm and the scoped Node gate. If it finds a failure,
reopen the responsible implementation task, fix and verify the affected scope,
then repeat Task 24.
Tasks 27 and 86 reuse those reports only when source, binaries, configuration
and inventories are unchanged.

Wasm lowering uses the completed VM foundation; tasks 07/10 own required fixes
without making their full historical plans prerequisites. Wasm suite closure
does not wait for Node profile qualification. Task 24 still requires all three
suite closures together.

Task 24 depends on the three suite closures. Support tasks supply required fixes
and checks, including existing specialization fallbacks, roots, host re-entry
and exhaustion; completing their historical plans is not an extra gate. Phase
task lists identify ownership, not mandatory implementation checklists.

**Only task 86 closes Stage A and permits Stage B.** Partial or historical
passes do not establish that gate. Once verified, proceed directly to tasks
62/61.

## Stage B — performance

After Stage A, run every V8-v7 benchmark on Apple M4/macOS: Crypto, DeltaBlue,
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
