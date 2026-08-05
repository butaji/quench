# Test262 Runner Integrity Plan

Test262 is the conformance oracle. The runner must execute the selected
corpus without rewriting fixtures, hiding failures, or treating skipped cases
as passes. Unit tests cover runner invariants and regressions; they do not
duplicate Test262 assertions.

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
