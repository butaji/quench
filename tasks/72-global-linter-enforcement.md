# Task 72: Enforce linter rules on every `*.rs` file

## Goal

The build script now lints every Rust source file in the workspace (not just `src/` and `crates/quench-runtime/src/`). Any violation of the project limits blocks the build.

## Rules

- **File length:** max 500 lines.
- **Function length:** max 40 lines.
- **Cyclomatic complexity:** max 10.

## Current state

- `build.rs` was updated to walk the whole workspace, skipping `target/`, `.git/`, `node_modules/`, and `dist/`.
- `build.rs` itself was refactored to comply with the same rules.
- Several legacy files currently exceed the limits but use `#![allow(file_length, ...)]` to opt out. Those files are skipped by the linter.

## Files that still opt out

- `crates/quench-runtime/src/interpreter.rs`
- `crates/quench-runtime/src/lower.rs`
- `crates/quench-runtime/tests/conformance.rs`
- `crates/quench-runtime/tests/project.rs`
- `crates/quench-runtime/tests/test262.rs`
- `crates/quench-runtime/src/builtins.rs`
- `crates/quench-runtime/src/lib.rs`
- `crates/quench-runtime/src/swc_parse.rs`
- `crates/quench-runtime/src/test262/harness.rs`
- `crates/quench-runtime/src/test262/runner.rs`
- `crates/quench-runtime/src/value.rs`
- `crates/quench-runtime/src/conformance/typescript/mod.rs`
- `crates/quench-runtime/src/conformance/typescript/directives.rs`
- `crates/quench-runtime/src/conformance/typescript/skip.rs`

(Verify the exact list by temporarily removing the allow attributes and running `cargo check`.)

## Next steps

1. For each opt-out file, either:
   - Split it into smaller modules/functions that meet the limits, then remove the allow attribute.
   - Keep the allow attribute with a documented justification if the file is generated or truly exceptional.
2. Do not add new opt-outs without justification.
3. Run `cargo check` after each change to confirm the build still passes.

## Verification

```bash
cargo check
cargo check -p quench-runtime
```

Both must pass. If a build fails, the error message lists the violating file, line, rule, and measured value.
