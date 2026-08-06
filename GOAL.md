# Goal: reach 100% Test262 as fast as possible

Quench reaches 100% Test262 conformance with the smallest safe Rust core and
self-hosted JavaScript layer, preserving TDD, runner integrity, minimum-LOC,
and no-GitHub-CI rules.

The optimization target is:

```text
time from failure discovery to verified canonical fix
```

## Operating strategy

1. Run digest mode to discover representative failure groups. The current
   `TEST262_QUICK=1` runner mode reduces retained failure output, but does not
   bound serial execution when cases pass.
2. Convert each distinct root cause into a failing Rust unit-test reproducer.
3. Fix the canonical spec-op, storage path, interpreter behavior, or owned JS
   builtin with the smallest change.
4. Run the focused unit suite and affected stage.
5. Run the complete stage digest, formatting, and clippy.

Quick mode changes diagnostic collection only. A complete digest with zero
failures and zero skips is the only conformance evidence.

Prioritize expected failures cleared per hour. Canonical operations and shared
engine paths outrank local symptoms. The R18–R21 direction is the highest
leverage: `SameValueZero` symbol identity, Map/Set internal storage leakage,
`loose_eq`/`ToPrimitive` canonicalization, and `__ops__` bridge divergence.
R22 builtin migration is opportunistic, not a throughput objective: migrate a
family only when failure fan-out or LOC data shows leverage. Runner throughput
work (R30–R34) runs in parallel. The execution and IR work is limited to the
interpreter-first Phase 3 boundary; Cranelift, JIT/AOT, and advanced garbage
collection remain future considerations.

No current stage is recorded anywhere; the milestones log is stale and is not
evidence. The first move of any session is a fresh complete digest from the
lowest failing stage (starting at stage 0) to establish ground truth.

## Instruments

- Test262 digest output as the conformance SSOT;
- quick digest mode for representative failure discovery;
- root-cause failure grouping;
- `run-test` and `inspect-test` for single-case diagnosis;
- focused Rust unit tests as the TDD reproducer gate;
- isolated execution and `run-each.sh` for crash-safe verification;
- explicit `TEST262_STAGE=N` selection (the `current-stage`/`advance-stage`
  script family is legacy and must not be treated as progress state);
- phase timing for discovery, bootstrap, parsing, execution, and cleanup;
- worker benchmarks using wall time, throughput, memory, timeout, and crash rate;
- ownership and lint checks to prevent duplicate implementations and debt.

## Acceleration targets

1. Make failure groups correspond to root causes rather than raw error strings.
2. Tune digest worker count from measurements with a bounded configurable cap.
3. Reduce repeated context/bootstrap work through immutable artifact caching.
4. Run independent stages concurrently with isolated results and serialized
   advancement.
5. Keep the unit-test-to-stage loop available through one local fast-loop tool.
6. Continue self-hosted builtin migration by family, measuring failures cleared,
   LOC changed, and duplicate ownership removed.

The practical engineering target is 2× faster feedback first, then 3–5× total
throughput after bootstrap optimization and safe stage-level concurrency. These
are targets, not conformance claims.

## Roadmap through Phase 3

The active roadmap ends at an interpreter-first IR:

1. Establish a fresh complete conformance and performance baseline.
2. Finish semantic blockers and the Test262/runtime boundary work.
3. Instrument the runner, measure RSS and phase costs, and improve bounded
   worker throughput without compromising isolation.
4. Introduce explicit heap and execution-state boundaries where they reduce
   ownership and allocation ambiguity.
5. Add a compact IR and IR interpreter, with differential tests against the
   existing AST evaluator before changing the default execution path.

The Phase 3 composition target is a configurable runtime boundary:

```text
Runtime<Heap, Collector, Allocator, Frames, Executor, Exceptions, Environments>
```

Each strategy owns one concern, while the first concrete implementation may
remain backed by the current reference-counted storage and a no-op collector.
Completion/control-flow state belongs behind `Exceptions`; it includes normal,
throw, return, break, continue, and suspension outcomes.

Execution-model expansion, moving or generational GC, Cranelift compilation,
JIT/AOT modes, inline caches, and deoptimization are future considerations.
They are not Phase 3 deliverables or current conformance gates.

## Non-negotiable proof

- Never edit `tests/test262`.
- Never add or restore GitHub Actions.
- Never treat quick mode, a partial batch, a stale catalog, or a skipped test as
  completion evidence.
- Never patch production code before the failing unit test exists and has failed.
- Never replace a canonical spec operation with a local copy.
- Never trade realm hygiene, panic freedom, deterministic digests, or isolation
  for speed.

Completion means every configured Test262 stage has a complete digest with zero
failures and zero skips, followed by unit, formatting, and clippy verification.
