# Test262 Runner Integrity Plan

Test262 is the conformance oracle. Release verification executes the pinned
ECMA-262 `test/` corpus with zero failures and zero skips, without rewriting
fixtures or hiding failures. Unit tests cover runner invariants and regressions;
they do not duplicate Test262 assertions.

## Invariants

- Load upstream harness files verbatim, apart from the documented
  `propertyHelper.js` compatibility swizzle.
- Parse metadata into one explicit model, including flags and negative
  expectations. Malformed metadata is a runner failure.
- Route every execution mode through the same timeout and outcome-checking
  contract.
- Keep process isolation as the default; in-process execution is opt-in.
- Emit deterministic digests containing stage identity, per-test outcomes,
  grouped failure reasons, skips, and timing.
- Treat every skipped test as incomplete. The runner must not convert a skip
  into a pass.
- Keep stage selection derived from the runner's stage list and never store
  test results in `tasks/` or `docs/`.

## Workflow

1. Run the selected stage with `TEST262_DIGEST=1`.
2. Group failures by normalized reason.
3. Add a failing Rust unit test for each distinct runner or runtime bug.
4. Make the smallest fix and verify the unit test, suite, formatting, and
   clippy.
5. Rerun the affected Test262 stage and use its output as the result.

The runner must remain local-only. GitHub Actions are forbidden in this
repository.

## Acceleration work

Runner improvements must preserve complete fixture execution, deterministic
outcomes, timeout enforcement, and skip accounting.

- Add phase timing for discovery, metadata/harness loading, context/bootstrap,
  parse, execution, microtask drain, cleanup, and worker startup.
- Use a persistent worker pool with configurable bounded worker count and
  benchmark it against throughput, memory, timeout rate, and crash rate.
- Improve grouping with stable phase, error type, runtime location, mode,
  builtin/abstract operation, and normalized-message fields, then rank groups
  by estimated affected tests per hour.
- Cache only immutable parsed harness/builtin artifacts. Mutable realm state,
  jobs, thrown values, and globals remain worker-local until reset tests prove
  reuse safe.
- Support concurrent independent stages using isolated result files and a
  serialized merge/advance operation.
- Provide triage, verify, and release modes. Triage may stop after
  representative root-cause groups; verify runs the complete affected stage;
  release runs every stage with zero skips.

Quick mode accelerates diagnosis only; a full digest remains the only
completion evidence.
