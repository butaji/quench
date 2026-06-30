# Task 50: Document how to run TypeScript's own test suite from the Quench repo

## Goal

Write a dedicated guide for contributors and CI that explains how to run TypeScript's test runners from inside the Quench repo.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: document the filtered runner commands used by Quench, not every possible TypeScript test flag.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `docs/typescript-tests.md` (new)
- `README.md`
- `EXECUTE.md`

## Document sections

1. **Setup**
   - `git submodule update --init tests/typescript`
   - `cd tests/typescript && npm ci`
2. **Build**
   - `npm run build:tests`
3. **Run filtered suites**
   - `npx hereby runtests --no-lint --runner=conformance`
   - `npx hereby runtests --no-lint --runner=compiler`
   - `npx hereby runtests --no-lint --runner=project`
   - `npx hereby runtests --no-lint --runner=transpile`
4. **Run a single test**
   - `npx hereby runtests --no-lint --runner=compiler --tests=2dArrays --dirty`
5. **Parallel runs**
   - `npx hereby runtests-parallel --no-lint --runner=compiler,conformance,project,transpile`
6. **Sharding**
   - `--shards=4 --shardId=1`
7. **Baselines**
   - `tests/baselines/reference/` vs `tests/baselines/local/`
   - `npx hereby diff`
   - `npx hereby baseline-accept`
8. **Troubleshooting**
   - timeouts, out-of-memory, missing baselines

## Boundaries

- Only modify documentation files.
- Do not modify `tests/typescript/`.

## Acceptance criteria

- `docs/typescript-tests.md` exists and a contributor can follow it to run the reference runners.
- `README.md` links to the new doc.

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.
- In CI, set per-test and job-level timeouts (e.g., 5 minutes per test suite, 30 minutes per job).


## Verification

```bash
ls docs/typescript-tests.md
```
