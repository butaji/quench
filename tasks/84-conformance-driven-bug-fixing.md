> **Workflow for turning conformance failures into fixed runtime bugs with regression tests.**

# Task 84: Conformance-driven bug fixing workflow

## Goal

Use the harness reports to pick the highest-impact failures, reproduce each one as a focused unit test, fix the runtime, and verify the failure bucket shrinks.

## Workflow

1. **Run a harness subset** to get the current failure landscape.
   ```bash
   cargo test -p quench-runtime --test conformance test_typescript_conformance_expressions -- --ignored --nocapture
   cargo test -p quench-runtime --test test262 test262_expressions -- --ignored --nocapture
   ```

2. **Open the Markdown report** for the run you care about:
   - `target/conformance_expressions_report.md`
   - `target/test262_report.md`

3. **Pick the top failure signature** you want to fix. Look for:
   - A clear runtime error (`ReferenceError`, `TypeError`, `is not a function`, etc.).
   - Many occurrences (high count).
   - A single root cause that explains the whole bucket.

4. **Find a minimal reproduction** from the example path in the report.
   - Open the file listed in the `Example` column.
   - Reduce it to the smallest JS/TS that still fails in `quench-runtime`.

5. **Write a focused unit test** in `crates/quench-runtime/tests/`.
   - Name it after the bug, e.g. `test_void_operator_class_reference`.
   - The test must fail before the fix and pass after.
   - Do not modify `tests/typescript/` or `tests/test262/`.

6. **Fix the runtime** with the smallest change that makes the new unit test pass.
   - Allowed: `crates/quench-runtime/src/` and `Cargo.toml`.
   - Not allowed: `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`, `examples/`.

7. **Run the unit test suite**:
   ```bash
   cargo test -p quench-runtime
   ```

8. **Re-run the harness subset** and confirm the bucket count dropped.
   - The Markdown/JSON report should show fewer occurrences of the fixed error signature.

9. **Commit** the unit test + fix together with a message like:
   ```
   fix: handle void operator on class static members
   - Adds regression test test_void_operator_class_reference
   - Fixes class declaration hoisting in lowerer
   ```

10. **Move to the next bucket.**

## Rules

- Every runtime fix must be accompanied by a unit test.
- Do not fix failures by changing harness skip rules unless the feature is genuinely not runnable.
- Do not edit external test fixtures.
- Prefer fixing one root cause that removes many failures over one-off hacks.

## Current top buckets to investigate

From the latest TypeScript expressions run:

| Count | Signature | Likely root cause |
|------:|-----------|-------------------|
| 37 | `ReferenceError: A is not defined` | Class/namespace declarations in baselines not evaluated or hoisted correctly |
| 37 | `No baseline found` | Baseline lookup/extraction gap |
| 20 | `ReferenceError: a is not defined` | Variable binding or parameter scoping |
| 14 | `ReferenceError: a1 is not defined` | Same as above |
| 13 | `is not a function` | Missing or incorrectly bound built-in method |
| 8 | `ReferenceError: o1 is not defined` | Optional-chain/nullish-coalescing variable resolution |

## Verification

After each fix:

```bash
cargo test -p quench-runtime
cargo test -p quench-runtime --test conformance test_typescript_conformance_expressions -- --ignored --nocapture
```

## Status

`pending` — this is a living workflow task. Close it when the conformance pass rate stops improving from obvious buckets.
