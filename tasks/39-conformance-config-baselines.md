# Task 39: Resolve configuration-specific baselines by directives

## Goal

Pick the correct baseline file when a TypeScript test case uses directives that change the emitted JS (e.g., `@target`, `@module`, `@jsx`).

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: handle `target` and `module` first; add other vary-by options as needed.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Background

TypeScript's `CompilerTest` constructs baseline names as:

```
<stem>(<key1>=<value1>,<key2>=<value2>...).<ext>
```

Keys are sorted alphabetically, values are lowercased, and the original extension is preserved inside the parentheses. Examples:

- `abstractPropertyBasics(target=es5).js`
- `commentsOnJSXExpressionsArePreserved(jsx=preserve,module=commonjs,moduledetection=auto).errors.txt`

## Files

- `crates/quench-runtime/tests/conformance.rs`

## Steps

1. Define the set of "vary-by" options: `target`, `module`, `jsx`, `strict`, `isolatedModules`, `downlevelIteration`, etc.
2. From the parsed directives, build an ordered map of the vary-by options present in the file.
3. Construct the configured basename:
   - If no vary-by options are present, use `<stem>.js`.
   - Otherwise use `<stem>(<key>=<lowercase-value>,...).js` with keys sorted alphabetically.
4. Look for the configured baseline first; fall back to the plain baseline if not found.
5. Add unit tests for a few representative cases.

## Boundaries

- Only modify test harness code.
- Do not modify `tests/typescript/`.

## Acceptance criteria

- A case like `abstractPropertyBasics` with `@target: es5` resolves to `abstractPropertyBasics(target=es5).js`.
- A case with no vary-by directives resolves to the plain `.js` baseline.

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.
- In CI, set per-test and job-level timeouts (e.g., 5 minutes per test suite, 30 minutes per job).


## Verification

```bash
cargo test -p quench-runtime --test conformance
```
