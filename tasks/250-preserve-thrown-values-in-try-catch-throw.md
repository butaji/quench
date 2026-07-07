# Task 250: Preserve thrown values in try/catch/throw

## Status: PENDING

## Gap

Thrown values are sometimes stringified or wrapped inconsistently, so `try/catch` cannot preserve the original error type and message. This breaks negative tests that expect specific error constructors.

## Fix

- Ensure `throw expr` preserves the exact value.
- Ensure `catch (e)` binds the original thrown value.
- Avoid implicit `toString` conversion when re-throwing.

## Acceptance criteria

- [ ] `try { throw new TypeError('x'); } catch (e) { e instanceof TypeError; }` is true.
- [ ] Caught error message is preserved.
- [ ] Regression tests for thrown value preservation.
- [ ] test262 negative tests that expect specific error types pass.

## Guardrail

Without this fix, the harness cannot honestly report 100% on error/exception suites because negative tests will fail for the wrong reason. This task is a blocker to closing Task 316 and Task 296.

## Files

- `crates/quench-runtime/src/interpreter/eval_stmt.rs`
- `crates/quench-runtime/src/interpreter/eval_expr.rs`
- `crates/quench-runtime/src/value.rs`

## Tests unblocked

- test262 negative tests expecting specific error types
- Error handling scenario tests

## Targets

- **Suite:** `both`
- **Batch:** 0
- **Target subset:** test262 / TypeScript error/exception subsets.
- **Blocked by:** see `tasks/index.json`
- **Exit criteria:** `throw`, `catch`, and re-throw preserve original error values and types; related negative tests pass at 100%.
