# Incremental test262 Conformance Design

## Objective

Drive Quench to full conformance with every test in the checked-in test262 submodule. Progress must be incremental, test-driven, minimal, and verified. No new test skips are permitted, and existing skip mechanisms must be removed as their underlying features become supported.

## Starting State

The repository already contains uncommitted runtime changes. They are protected work: do not discard, overwrite, or silently fold unrelated edits into a change. Validate them first and commit them only when their behavior is coherent and verified.

The staged runner is fail-fast and checkpointed. It remains the primary feedback loop because it exposes the earliest unsupported behavior and resumes after each fix.

## Delivery Loop

Each semantic increment follows the same sequence:

1. Run the smallest relevant test262 test or batch.
2. Capture the required behavior in a focused Rust regression test.
3. Confirm that the regression test fails for the intended reason.
4. Implement the minimum engine change that satisfies the behavior.
5. Run the regression test, relevant test262 test or batch, formatting, and Clippy.
6. Commit only the verified increment.
7. Resume at the next test262 failure.

Failures are diagnosed through executable tests rather than temporary logging or speculative changes.

## Parallel Work

Subagents may investigate independent test failures, inspect specification behavior, propose regression tests, or review completed changes in parallel. Shared evaluator and runtime files have one coordinated editing stream at a time. This preserves existing work and avoids conflicting patches.

## Skip Removal

No new skips may be added. Existing feature, flag, source, path, module, and tolerated-harness skips are an explicit backlog. A skip is removed when the corresponding implementation has focused regression coverage and the newly enabled test262 tests pass. The final completion condition requires every test from the submodule to execute without any runner skip or tolerated harness failure.

## Lint Constraints

The target constraints for every Rust source file are:

- At most 500 physical lines per file.
- At most 40 physical lines per function.
- Cognitive complexity at most 10.

Clippy enforces function length and cognitive complexity. A dependency-free repository script enforces file length because Clippy has no file-length lint. Existing violations are reduced when their files are touched and then eliminated systematically. Hard repository-wide enforcement is enabled only after the legacy baseline reaches zero, so conformance work can begin immediately without hiding violations or introducing broad lint exceptions.

Broad lint suppressions are not a substitute for compliance. New or modified code must meet the limits immediately.

## Architecture and Scope

Language behavior stays in the existing parser, lowering, evaluator, environment, value, and builtin boundaries. Fixes use existing project patterns and dependencies. New abstractions are introduced only when necessary to keep files and functions within limits or when multiple supported semantics need the same operation.

Runner changes are limited to making all tests executable and accurately reporting results. The runner must not alter test semantics, manufacture passes, or suppress unsupported behavior.

## Error Handling

JavaScript-visible failures use the runtime's `JsError` and thrown-value mechanisms. Builtins must not panic. Unsupported semantics remain failing tests until implemented; they are never converted into runner skips.

## Verification

Every committed increment must pass:

- Its focused Rust regression test.
- The affected test262 test or smallest relevant batch.
- `cargo fmt --all -- --check`.
- `cargo clippy -p quench-runtime --all-targets -- -D warnings`.

Broader crate tests run when the change affects shared semantics. Periodic full-stage and full-suite runs detect regressions and establish measurable progress. Final completion requires the entire submodule to run with zero failures and zero skipped or tolerated tests, plus repository-wide lint-limit enforcement.

## Commit Policy

Each successful semantic step receives a focused commit after verification. Existing uncommitted work is never committed blindly: it must first pass tests and be separable as a coherent step. Git history should make each conformance improvement independently reviewable.
