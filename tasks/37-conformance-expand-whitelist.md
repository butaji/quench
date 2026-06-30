# Task 37: Expand conformance whitelist as runtime support grows

## Goal

Keep the conformance whitelist in sync with runtime capabilities. Add new categories once the runtime can execute them meaningfully.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: only add a category when it is expected to have a non-zero pass rate.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `crates/quench-runtime/tests/conformance.rs` (`WHITELIST_DIRS`)
- `crates/quench-runtime/src/lower/`
- `crates/quench-runtime/src/interpreter/`

## Categories to add in order

1. **Modules** — `conformance/modules/` and `conformance/es6/moduleExports/` once `import`/`export` lowering works.
2. **Async / await** — `conformance/async/` and `conformance/es2017/asyncIterators/` once async functions are supported.
3. **Generators** — `conformance/generators/` and `conformance/es2018/` once generators/yield work.
4. **Decorators** — `conformance/decorators/` once decorator metadata helpers are available.
5. **JSX** — `conformance/jsx/` once a JSX runtime stub is provided.
6. **Namespaces** — `conformance/namespaces/` once namespace lowering is implemented.

## Steps

1. After each major feature lands, run the harness against the corresponding category.
2. If the category has a meaningful pass rate, add it to `WHITELIST_DIRS`.
3. Update the conformance report with the new category's results.

## Boundaries

- Only modify the whitelist and reports.
- Do not modify `tests/typescript/`.

## Acceptance criteria

- Each new category added to the whitelist has at least one passing case.
- The CI threshold (Task 36) is updated when categories are added.

## Verification

```bash
cargo test -p quench-runtime --test conformance -- --nocapture
```
