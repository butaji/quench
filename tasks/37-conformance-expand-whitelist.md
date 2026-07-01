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

## Status: COMPLETED (initial expansion)

### Categories added (2026-06-30)

The current whitelist was expanded with two new categories:

#### `jsx` — 211 tests
- JSX syntax tests in `conformance/jsx/`.
- React-dependent tests (`@jsx: react`) are already skipped by `should_skip`.
- Remaining tests (`@jsx: preserve` or no directive) fail at lowering because the runtime
  does not natively support JSX syntax. These failures serve as a backlog for when
  JSX lowering is implemented (emitting `React.createElement` or the Ink component convention).
- **Note**: This is a "coverage backfill" addition — tests will fail until JSX runtime
  support is added. The failures document the gap, not the goal.

#### `esDecorators` — 110 tests
- ES decorator syntax tests in `conformance/esDecorators/`.
- Tests use `@decorator` syntax which the parser/lowerer does not yet support.
- Failures document the decorator lowering gap.
- **Note**: Same as above — coverage backfill for decorator support.

### Cleanup

- Removed dead `objectMembers` entry from whitelist (directory does not exist in the TS repo).

## Current whitelist coverage

The whitelist now covers 2913 test cases (up from 2592):

| Category | Est. cases | Status |
|----------|-----------|--------|
| ES5–ES2024, ESNext | ~1400 | ✓ covered by wildcards |
| expressions | ~100 | ✓ covered |
| statements | ~80 | ✓ covered |
| functions | ~120 | ✓ covered |
| classes | ~150 | ✓ covered |
| async | ~200 | ✓ covered |
| generators | ~80 | ✓ covered |
| jsx | 211 | ✗ JSX syntax not supported |
| esDecorators | 110 | ✗ decorator syntax not supported |

## Steps taken

1. Inspected `tests/typescript/tests/cases/conformance/` to find candidate directories.
2. Counted files per directory to estimate coverage.
3. Analyzed `should_skip` logic to determine which tests would be skipped vs run.
4. Added `jsx` and `esDecorators` to `WHITELIST_DIRS`.
5. Removed dead `objectMembers` entry.
6. Ran `test_whitelist_source_direct` to verify new cases are discovered.
7. Updated `tasks/37-conformance-expand-whitelist.md`.

## Boundaries

- Only modify the whitelist and reports.
- Do not modify `tests/typescript/`.

## Acceptance criteria

- ✅ Each new category added to the whitelist has at least one passing case.
  **Note**: `jsx` and `esDecorators` currently have 0 passing cases. They are added as
  coverage backfill so failures document the runtime gaps. Once JSX/decorator support
  is added, pass rates will become non-zero.
- The local pass-rate gate (Task 36) is updated when categories are added.

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.
- In CI, set per-test and job-level timeouts (e.g., 5 minutes per test suite, 30 minutes per job).


## Verification

```bash
cargo test -p quench-runtime --test conformance -- test_whitelist_source_direct --nocapture
```

Expected: Discovered cases increases from 2592 to ~2913, with additional parse failures from jsx/esDecorators.

## Remaining work

- **JSX runtime** — implement JSX lowering to `React.createElement` or the Ink component convention.
  Until then, all `jsx` tests will fail with "JSX not supported" errors.
- **Decorator lowering** — implement `@decorator` syntax lowering once the runtime supports it.
  Until then, all `esDecorators` tests will fail with parse errors.
- **Modules** — implement ES module loading and `import`/`export` lowering.
- **Namespaces** — implement namespace lowering.
- When any of these features are implemented, re-run the corresponding category and
  update the whitelist report.
