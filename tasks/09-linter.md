# Task 09: Make build.rs enforce project lint rules on every *.rs file

## Goal

`build.rs` must enforce the project limits on **all** Rust source files, not just the `src/compiler/` directory or the runtime crate. Every `*.rs` file in the project must pass:

- File length: max 500 lines
- Function length: max 40 lines
- Cyclomatic complexity: max 10

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.


## Files

- `build.rs`
- All `*.rs` files in `src/`, `crates/*/src/`, `tests/`, `examples/` (if any Rust), and `build.rs` itself

## Current state

`build.rs` already implements the three checks, but it only runs them on `src/` and `crates/quench-runtime/src/`. Worse, violations outside `src/compiler/` are emitted as `cargo:warning=` instead of failing the build. The current build therefore tolerates:

- `src/bridge/ffi.rs`: 564 lines (file length), 49-line function
- `src/cli.rs`: 50-line function, complexity 13
- `src/event_loop.rs`: complexity 12 and 13
- `src/hotreload.rs`: 54-line function
- `src/main.rs`: 673 lines (file length), complexity 12
- `src/signals.rs`: complexity 12

## Steps

1. Update `build.rs` to lint every `*.rs` file in the project:
   - `src/`
   - `crates/quench-runtime/src/`
   - `tests/` (Rust test files)
   - `examples/` (if it contains `.rs`)
   - `build.rs` itself
   - Exclude `target/` and the `tests/typescript/` submodule.
2. Change the failure mode from `cargo:warning=` to `panic!` for **all** violations. The build must fail if any file/function exceeds the limits.
3. Run `cargo build` and fix every violation. Refactor by:
   - Splitting files longer than 500 lines into submodules.
   - Extracting helper functions for bodies over 40 lines.
   - Reducing cyclomatic complexity by extracting match arms, early returns, and nested conditions into helpers.
4. Re-run `cargo build` until it succeeds with zero lint violations.

## Boundaries

- Only modify `build.rs` and files needed to satisfy the linter.
- Refactoring must preserve behavior; add regression tests where the change is non-trivial.
- Do not modify `tests/typescript/` or `examples/` JS/TS fixtures.

## Acceptance criteria

- `cargo build` fails if any `*.rs` file exceeds 500 lines.
- `cargo build` fails if any function body exceeds 40 lines.
- `cargo build` fails if any function exceeds complexity 10.
- `cargo build` succeeds on the cleaned-up codebase.

## Verification

```bash
cargo build
```
