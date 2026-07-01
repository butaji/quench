# Task 72: Enforce linter rules on every `*.rs` file

## Goal

The build script enforces the project limits on every Rust source file in the workspace. No `#[allow(...)]` attribute or other opt-out bypasses the linter. Any violation blocks the build.

## Rules

- **File length:** max 500 lines.
- **Function length:** max 40 lines.
- **Cyclomatic complexity:** max 10.

## Scope

- All `*.rs` files under the repository root are linted.
- Skipped directories: `.git/`, `target/`, `node_modules/`, `dist/`, and the conformance submodules `tests/test262/` and `tests/typescript/`.
- `build.rs` itself is linted and must comply with the same rules.

## No opt-outs

- `#[allow(file_length)]`, `#[allow(clippy::function_length)]`, `#[allow(clippy::complexity)]`, `#[allow(unknown_lints)]`, and similar attributes are **not honored** by the build linter.
- Files that currently exceed the limits will cause `cargo check` / `cargo build` to fail until they are refactored or split.

## Fixing violations

- Refactor oversized files into smaller modules.
- Split long functions into helper functions that each do one thing.
- Reduce complexity by extracting early returns, helper functions, or match arms.
- Do not add new `#[allow(...)]` attributes to bypass the linter.

## Verification

```bash
cargo check
cargo check -p quench-runtime
```

If the build fails, the error message lists every violating file with the rule and measured value.
