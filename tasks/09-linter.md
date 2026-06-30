# Task 09: Extend build.rs linter to quench-runtime and enforce limits

## Done

- `build.rs` now lints both `src/` and `crates/quench-runtime/src/` (already done).
- Linter was updated to handle Rust closures (don't count closure bodies as part of outer function body).
- Many unused imports and code warnings fixed in quench-runtime.

## Goal

Make sure the custom runtime code is checked by the same linter rules as the rest of the project.

## Background

`build.rs` already lints Rust sources under `src/` against:

- File length: max 500 lines
- Function length: max 40 lines
- Cyclomatic complexity: max 10

However, it currently only scans `src/`, so `crates/quench-runtime/src/` is not linted at all.

## Files

- `build.rs`
- `crates/quench-runtime/src/*.rs` (any files that violate the limits after extending the linter)

## Steps

1. In `build.rs`, change `run_linter` to also lint `crates/quench-runtime/src`:
   ```rust
   fn run_linter() {
       for dir in [Path::new("src"), Path::new("crates/quench-runtime/src")] {
           if let Err(e) = lint_rust_sources(dir) {
               for line in e.lines() {
                   println!("cargo:warning={}", line);
               }
           }
       }
   }
   ```
2. Run `cargo build` and collect all linter warnings for `crates/quench-runtime/src/`.
3. Refactor any violating functions/files:
   - Split functions longer than 40 body lines into smaller helpers.
   - Reduce cyclomatic complexity above 10 by extracting match arms or early returns into helper functions.
   - Split files longer than 500 lines into submodules (e.g., `lower/expr.rs`, `lower/stmt.rs`).
4. Re-run `cargo build` until no linter warnings remain for `crates/quench-runtime/src/`.

## Boundaries

- Only modify `build.rs` and `crates/quench-runtime/src/` for refactoring/lint compliance.
- Do not change behavior while refactoring.
- Do not touch `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`.
- `examples/` are immutable.

## Acceptance criteria

- `cargo build` produces no linter warnings for files in `crates/quench-runtime/src/`.
- All runtime source files are ≤ 500 lines.
- All runtime functions have body ≤ 40 lines and cyclomatic complexity ≤ 10.

## Verification

```bash
cargo build
```
