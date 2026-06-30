# Task 29: Run TypeScript conformance harness over whitelist and report results

## Goal

Execute the conformance harness over the curated whitelist, record pass/fail/skip for every case, and produce a prioritized backlog that feeds into bug-fix tasks.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: focus on directories that exercise the features Quench already claims to support.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `crates/quench-runtime/tests/conformance.rs`
- `crates/quench-runtime/tests/conformance/runner.rs`
- `tasks/26-review-findings.md`
- `tasks/index.json`

## Whitelist

Run the harness against:

```text
conformance/es6/
conformance/es7/
conformance/es2016/
conformance/es2017/
conformance/es2018/
conformance/es2019/
conformance/es2020/
conformance/es2021/
conformance/es2022/
conformance/es2023/
conformance/es2024/
conformance/esnext/
conformance/expressions/
conformance/statements/
conformance/functions/
conformance/classes/
conformance/enums/
conformance/constEnums/
conformance/async/
conformance/asyncGenerators/
conformance/generators/
conformance/controlFlow/
conformance/emitter/
```

## Steps

1. Add a test entry point in `conformance.rs` that calls `run_conformance(&whitelist)`.
2. Run the harness with `--nocapture` and collect:
   - total cases discovered
   - skipped (with reason)
   - passed
   - failed (with file path and error message)
3. Bucket failures by feature using the file path:
   - expressions, statements, functions, classes, iterators, modules, async, generators, control flow, enums, emitter
4. For each bucket, list the top 5 representative failing files.
5. Cross-reference failures with Task 26 findings and existing tasks (14, 17, 18, 19, 21).
6. Update `tasks/index.json` notes with pass/fail/skip counts and add follow-up tasks for any new categories.
7. Write a short `docs/conformance-report.md` (optional; only if requested) summarizing the results.

## Output format

```text
TypeScript conformance whitelist run
=====================================
Total:    1847
Skipped:  623 (unsupported module: 120, has errors: 340, no baseline: 163)
Passed:   412
Failed:   812

expressions: 312 failed
  - expressions/asOperator/asOperatorAmbiguity.ts: TypeError ...
  - expressions/optionalChaining/...: LowerError ...
statements: 205 failed
  ...
```

## Boundaries

- Do not modify `tests/typescript/`.
- Do not fix runtime bugs in this task; only collect and categorize failures.

## Acceptance criteria

- The harness runs to completion over the whitelist without panicking.
- A summary with counts per category is written into Task 29 notes or a report file.
- Every failure category is mapped to an existing or new task.

## Verification

```bash
cargo test -p quench-runtime --test conformance -- --nocapture > conformance.log 2>&1
```
