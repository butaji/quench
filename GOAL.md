# Goal: reach 100% Test262 as fast as possible

Quench reaches 100% Test262 conformance with the smallest safe Rust core and
self-hosted JavaScript layer, preserving TDD, runner integrity, minimum-LOC,
and no-GitHub-CI rules.

The optimization target is:

```text
time from failure discovery to verified canonical fix
```

## Operating strategy

1. Run quick digest mode to discover representative failure groups.
2. Convert each distinct root cause into a failing Rust unit-test reproducer.
3. Fix the canonical spec-op, storage path, interpreter behavior, or owned JS
   builtin with the smallest change.
4. Run the focused unit suite and affected stage.
5. Run the complete stage digest, formatting, and clippy.

Quick mode accelerates diagnosis only. A complete digest with zero failures and
zero skips is the only conformance evidence.

Prioritize expected failures cleared per hour. Canonical operations and shared
engine paths outrank local symptoms. The R18–R22 direction is high leverage:
equality, internal storage, `ToPrimitive`, `__ops__`, and builtin ownership
migration.

## Instruments

- Test262 digest output as the conformance SSOT;
- quick digest mode for representative failure discovery;
- root-cause failure grouping;
- `run-test` and `inspect-test` for single-case diagnosis;
- focused Rust unit tests as the TDD reproducer gate;
- isolated execution and `run-each.sh` for crash-safe verification;
- stage and pending-batch tools for prioritization;
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
