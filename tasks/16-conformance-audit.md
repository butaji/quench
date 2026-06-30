# Task 16: Audit TypeScript conformance tests and categorize runtime failures

## Goal

Run the conformance harness over a curated, runtime-relevant subset of the TypeScript conformance suite, bucket the failures by language feature, and produce a prioritized backlog.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: start with the whitelist directories that cover the most common JS/TS runtime features; skip type-check-only and exotic module systems.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `crates/quench-runtime/tests/conformance.rs`
- `tests/typescript/tests/cases/conformance/`
- `tests/typescript/tests/baselines/reference/`
- `tasks/` (this task and any follow-ups it spawns)

## First-pass whitelist

Start the audit with these directories; they are the most likely to expose runtime gaps without requiring module-loaders, JSX runtimes, or decorator helpers:

- `conformance/es6/`
- `conformance/es7/`
- `conformance/es2016/` through `conformance/es2024/`
- `conformance/esnext/`
- `conformance/expressions/`
- `conformance/statements/`
- `conformance/functions/`
- `conformance/classes/`
- `conformance/enums/`
- `conformance/constEnums/`
- `conformance/async/`
- `conformance/asyncGenerators/`
- `conformance/generators/`
- `conformance/controlFlow/`
- `conformance/emitter/`

## Skip rules

Skip a case if any of the following is true:

- A non-empty `.errors.txt` baseline exists for the chosen configuration.
- `// @noEmit: true` or `// @emitDeclarationOnly: true`.
- The chosen module system is unsupported (`amd`, `umd`, `system`, `node16`, `nodenext`).
- The case uses JSX and no JSX runtime stub is available.
- The case uses decorators/metadata and no helper stubs are available.
- The directory is type-check-only (`types`, `interfaces`, `Symbols`, `declarationEmit`, `additionalChecks`, `pedantic`, `jsdoc`, `salsa`, `typings`, `override`).

## Steps

1. Run the harness over the whitelist with `--nocapture`.
2. Categorize each failure by feature using the file path and error message:
   - `expressions/` — operators, member access, optional chaining, spread, template literals
   - `statements/` — var/let/const, loops, switch, try/catch, labels
   - `functions/` — default/rest/destructuring params, closures, `this`, `arguments`
   - `classes/` — constructors, `super`, inheritance, static members, accessors
   - `iterators/` — `for...of`, generators, iterables
   - `modules/` — `import`/`export` execution
   - `async/` — `Promise`, `async`/`await`
3. For each failure, record:
   - the input `.ts` path
   - the baseline `.js` path used
   - the runtime error or assertion mismatch
   - the likely missing feature or bug
4. Produce a table with counts per category and the top 3 representative failing files per category.
5. Update `tasks/index.json` and create follow-up task files if a category needs its own task.
6. Do not modify `tests/typescript/`.

## Boundaries

- Read-only exploration of `tests/typescript/`.
- No runtime code changes unless a one-line harness fix is required to collect data.

## Acceptance criteria

- A conformance summary is written into a task note or a `docs/` scratchpad.
- Every failing category maps to an open task or an existing Task 14/17/18/19 item.
- The harness runs to completion without panicking over the whitelist.

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.
- In CI, set per-test and job-level timeouts (e.g., 5 minutes per test suite, 30 minutes per job).


## Verification

```bash
cargo test -p quench-runtime --test conformance -- --nocapture > conformance.log 2>&1
```
