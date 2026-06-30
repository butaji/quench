# Task 33: Add TypeScript source-direct execution mode to conformance harness

## Goal

Make the harness execute the original `.ts`/`.tsx` source directly in `quench-runtime` instead of only running the pre-compiled baseline JS. The baseline JS becomes a fallback/verification mechanism.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: run source-direct for cases the runtime already parses; fall back to baseline JS for cases with unsupported TS syntax.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `crates/quench-runtime/tests/conformance.rs`
- `crates/quench-runtime/src/context/mod.rs`
- `crates/quench-runtime/src/lib.rs`

## Steps

1. Add a `run_mode` enum to the harness:
   - `BaselineJs` — current behavior: extract JS from baseline and run it.
   - `SourceTs` — run the raw `.ts`/`.tsx` source via `ctx.eval_ts()`.
   - `Hybrid` — try source first; if parsing/lowering fails, fall back to baseline JS.
2. Add a test entry point `test_whitelist_source_direct` that uses `SourceTs` or `Hybrid`.
3. Track source-direct failures separately from baseline failures.
4. Identify which failures are due to unsupported TypeScript syntax vs runtime bugs.
5. Update Task 21 (TypeScript stripping) with the list of syntax constructs that still fail.

## Boundaries

- Only modify test harness code and the public `eval_ts` API if needed.
- Do not modify `tests/typescript/`.

## Acceptance criteria

- The harness can run at least one simple `.ts` case source-direct and pass.
- A report compares source-direct vs baseline-js pass rates.

## Verification

```bash
cargo test -p quench-runtime --test conformance -- test_whitelist_source_direct --nocapture
```
