<!-- READ-ONLY FILE -->
<!-- This file is the canonical record of the project's Rust source linting rules. -->
<!-- Do not edit it directly. To change the rules, create a task and update this file in the same commit. -->

# Rust Source Linter Rules

## Status

**Enforced.** The rules below are checked by `build.rs` on every `cargo check` / `cargo build`. Any violation fails the build.

## Rules

| Rule | Limit |
|------|-------|
| File length | 500 lines |
| Function body length | 40 lines |
| Cyclomatic complexity | 10 |

## Scope

- Every `*.rs` file in the workspace is linted.
- Skipped directories: `.git/`, `target/`, `node_modules/`, `dist/`, `tests/test262/`, `tests/typescript/`.
- `build.rs` itself is also linted and must comply.

## No exemptions

- `#[allow(file_length)]`, `#[allow(clippy::function_length)]`, `#[allow(clippy::complexity)]`, `#[allow(unknown_lints)]`, and similar attributes are **not honored** by the build linter.
- There are no special-case waivers for individual files.

## Implementation

- `build.rs` uses `syn` to parse each Rust source file.
- `syn::visit` extracts `fn` items and measures body line count and cyclomatic complexity.
- Violations are reported with file path, line number, rule, and measured value.

## Verification

```bash
cargo check
cargo check -p quench-runtime
```

The build succeeds only when all violations are resolved.

## Fixing violations

- Split oversized files into smaller modules.
- Split long functions into focused helpers.
- Reduce complexity by introducing early returns, helper functions, or smaller match arms.
- Do not add new `#[allow(...)]` attributes to bypass the linter.
