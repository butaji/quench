# Task 17: Pass TypeScript expression and statement conformance tests

## Goal

Make the runtime pass all runtime-relevant expression and statement conformance cases.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## Files

- `crates/quench-runtime/src/lower/expr.rs`
- `crates/quench-runtime/src/lower/stmt.rs`
- `crates/quench-runtime/src/lower/helpers.rs`
- `crates/quench-runtime/src/interpreter/eval_expr/*.rs`
- `crates/quench-runtime/src/interpreter/eval_stmt/*.rs`
- `crates/quench-runtime/src/interpreter/binary_ops.rs`

## Steps

1. From the Task 16 audit, pick the expression/statement failures.
2. Implement or fix the missing features, which are likely to include:
   - `delete` operator
   - unary `+`, `~`, `void`
   - compound assignment operators (`+=`, `-=`, `??=`, `||=`, `&&=`)
   - comma operator
   - `switch` statement (with `break`)
   - labeled `break`/`continue`
   - `try`/`catch`/`finally`
   - `with` statement (may be intentionally unsupported; document if skipped)
3. Add a unit test for each fixed feature in `crates/quench-runtime/tests/`.
4. Re-run the expression/statement conformance subset and confirm the count of failures goes down.

## Boundaries

- Only modify `crates/quench-runtime/src/`.
- Do not touch `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`.
- Do not modify `tests/typescript/` or `examples/`.

## Acceptance criteria

- All expression/statement conformance cases that are in scope pass.
- Skipped features (`with`, strict-mode-only edge cases) are documented in `tasks/20-conformance-stabilize.md`.

## Verification

```bash
cargo test -p quench-runtime --test conformance -- expressions statements
```
