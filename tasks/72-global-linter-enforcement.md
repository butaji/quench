> **Status:** enforced. The build now fails until the listed violations are fixed.

> **Canonical rules are in `docs/linter-rules.md` (read-only).** This task tracks the implementation and cleanup work.

# Task 72: Enforce linter rules on every `*.rs` file

## Goal

The build script enforces the project limits on every Rust source file in the workspace. No `#[allow(...)]` attribute or other opt-out bypasses the linter. Any violation blocks the build.

## Rules (see `docs/linter-rules.md`)

- **File length:** max 500 lines.
- **Function length:** max 40 lines.
- **Cyclomatic complexity:** max 10.

## Implementation

- `build.rs` uses `syn` to parse every `*.rs` file in the workspace.
- Skipped directories: `.git/`, `target/`, `node_modules/`, `dist/`, and the conformance submodules `tests/test262/` and `tests/typescript/`.
- `#[allow(file_length)]`, `#[allow(clippy::function_length)]`, `#[allow(clippy::complexity)]`, `#[allow(unknown_lints)]`, etc. are **not honored**.
- `build.rs` itself is also linted and must comply with the same rules.

## Current violations (as of latest run)

`cargo check` currently reports **48 files scanned**, with the following categories of violations:

- **File length:** `builtins.rs` (1061), `interpreter.rs` (1022), `lower.rs` (977), `src/bridge/ffi.rs` (565), `src/main.rs` (633).
- **Function length:** multiple functions in `builtins.rs`, `interpreter.rs`, `lower.rs`, `src/bridge_reg.rs`, `src/main.rs`, `xtask/src/main.rs`, and several test files exceed 40 lines.
- **Complexity:** multiple functions exceed complexity 10, especially `register_builtins`, `eval_expression`, `lower_expr`, and `main`.

Run `cargo check` for the full, exact list.

## Fixing violations

- Refactor oversized files into smaller modules.
- Split long functions into helpers that each do one thing.
- Reduce complexity by extracting early returns, helper functions, or match arms.
- Do not add new `#[allow(...)]` attributes to bypass the linter.

## Verification

```bash
cargo check
cargo check -p quench-runtime
```

The build will succeed only when all violations are resolved.
