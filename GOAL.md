# Quench mission

Finish the architecture and every implementation task described by the
repository docs. Read `README.md`, `docs/`, and all of `tasks/` first; treat
them as the specification and keep their design coherent as the code evolves.

Build one machine-first OXC-facts residual VM: syntax and static meaning come
from OXC, facts describe what is proven/guarded/unknown, and runtime code
represents only remaining dynamic uncertainty. The runtime must be shaped for
instruction-cache locality, predictable branches, compact loads, low allocation
rate, and minimal peak RSS. Do not create parallel semantic worlds.

The performance end state is part of the goal, not a later optimization pass:

- flat encoded Ops and shared Code IDs, never nested runtime operation vectors;
- `HeapRef(u32)` values, shape IDs, packed slots, indexed environments, and
  fixed stack frames;
- bounded caches and reclaimable arenas for code, keys, metadata, and temporary
  compilation state;
- one generated mechanical declaration per value layout, Op, builtin, and
  intrinsic relation;
- explicit Completion transitions and materialized continuations only for live
  suspension state;
- benchmark evidence for cycles/op, branches, allocations, live bytes, and RSS
  before accepting a fast path.

Any design that adds a second semantic representation, duplicates metadata,
keeps an avoidable allocation in a hot path, or trades RSS for unmeasured speed
is incomplete even if its tests pass.

This scope has exactly one execution engine: the compact residual interpreter.
JITs, native lowering, native code caches, alternate execution modes, and
benchmark-only behavior are explicitly out of scope.

Think in Lisp:

- Make programs, semantics, state machines, reducers, and runtime operations
  uniform data wherever possible.
- Describe repeated mechanics once, then derive them with tables or Rust
  macros; do not hand-copy variants.
- Prefer composition and transformation of data over ad-hoc control flow.
- When failures repeat, improve the underlying representation or transition
  system instead of adding cases.

Work as a learning loop. Measure the current behavior, form a mechanism-level
hypothesis, implement the smallest general change, and attack it with nearby
and adversarial cases. Keep useful diagnostics and explanations, discard
temporary scaffolding, and leave the next worker with the clearest remaining
uncertainty. Choose whatever tools or experiments improve understanding; this
goal constrains outcomes, not the path.

Advance test262 in the order defined by `docs/STAGES.md`. Every discovered test
in stages 0–113 must execute through the canonical runner and pass, with no
skips or altered harness behavior. After each change, rerun the affected stage
and regression stages 0 through the current stage. Verify the complete range
again before claiming completion. Keep test262 itself an immutable input.

Completion requires source-level task coverage, 100% stable-stage results,
green workspace quality checks, and committed verified changes. Do not claim
success from a partial run, a workaround, or an untested assumption.
