# Task 16: Audit TypeScript conformance tests and categorize runtime failures

## Goal

Run the conformance harness, bucket the failures by language feature, and produce a prioritized backlog.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.


## Files

- `crates/quench-runtime/tests/conformance.rs`
- `tasks/` (this task and any follow-ups it spawns)
- Optional: `docs/conformance-audit.md` (scratchpad; do not commit unless requested)

## Steps

1. Run the harness over the full `tests/typescript/tests/cases/conformance/` tree with `--nocapture`.
2. Categorize each failure by feature using the file path and error message:
   - `expressions/` — operators, member access, optional chaining, spread, template literals
   - `statements/` — var/let/const, loops, switch, try/catch, labels
   - `functions/` — default/rest/destructuring params, closures, `this`, `arguments`
   - `classes/` — constructors, `super`, inheritance, static members, accessors
   - `iterators/` — `for...of`, generators, iterables
   - `modules/` — `import`/`export` execution
   - `async/` — `Promise`, `async`/`await`
3. Produce a table with counts per category and the top 3 representative failing files per category.
4. Update `tasks/index.json` and create follow-up task files if a category needs its own task.
5. Do not modify `tests/typescript/`.

## Boundaries

- Read-only exploration of `tests/typescript/`.
- No runtime code changes unless a one-line harness fix is required to collect data.

## Acceptance criteria

- A conformance summary is written into a task note or a `docs/` scratchpad.
- Every failing category maps to an open task or an existing Task 14/17/18/19 item.
- The harness runs to completion without panicking.

## Verification

```bash
cargo test -p quench-runtime --test conformance -- --nocapture > conformance.log 2>&1
```
