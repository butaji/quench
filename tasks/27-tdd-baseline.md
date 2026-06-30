# Task 27: Establish TDD baseline and unit test coverage

## Goal

Make TDD the default way of working and ensure every known bug and new feature is pinned by a unit test before the implementation is written.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: write regression tests for the blockers that prevent examples from running first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Current state

- `crates/quench-runtime/` has no dedicated `tests/` directory yet.
- `tests/parity.rs` in the main crate covers high-level integration but not runtime edge cases.
- Task 26 contains ~40 ranked findings, many of which can be reproduced with small unit tests.

## Steps

1. Create the test layout:
   ```
   crates/quench-runtime/tests/
   ├── parser_lower.rs       # swc_parse and lowering tests
   ├── interpreter.rs        # expression/statement evaluation tests
   ├── builtins.rs           # Array, Map, Set, String, Date, Object, JSON, Math, Promise
   ├── value.rs              # Value conversions, prototype chain, equality
   ├── reactive.rs           # Signal/Memo/Effect/Render once they exist
   └── conformance.rs        # TypeScript conformance harness (Task 15)
   ```
2. For each Rank 1 and Rank 2 finding in Task 26, write a failing unit test **before** fixing the bug:
   - event-loop dispatch / microtask draining (integration-level, may stay in `tests/parity.rs`)
   - array prototype linkage on builtin-created arrays
   - correct `==` and `instanceof`
   - `break`/`continue` propagation
   - arrow-function lexical `this`
   - `String.prototype.split`
   - object spread ignoring internal slots
   - error taxonomy and stack traces
   - native function `this` binding
   - inherited getters/setters
   - real `Function.prototype.call` / `apply`
   - `var` scoping
   - lowering of `break`/`continue`/class/import/export
3. Add unit tests for every new language feature as it is implemented (optional chaining, destructuring, classes, async, modules).
4. Add unit tests for each built-in method added or fixed.
5. Keep a `#[ignore]` test list for features that are intentionally deferred, with a comment linking to the relevant task.

## Test patterns

Use the existing `Context` API:

```rust
use quench_runtime::Context;

#[test]
fn array_map_returns_array_with_prototype() {
    let mut ctx = Context::new();
    let result = ctx.eval("[1,2,3].map(x => x * 2).map(x => x + 1)").unwrap();
    assert_eq!(result.to_string(), "3,5,7");
}
```

For lowering errors:

```rust
#[test]
fn break_statement_lowers() {
    let mut ctx = Context::new();
    let result = ctx.eval("for (let i = 0; i < 3; i++) { if (i === 1) break; } i");
    assert!(result.is_ok());
}
```

## Boundaries

- Only add tests in `crates/quench-runtime/tests/` and `tests/`.
- Do not modify `examples/` or `tests/typescript/`.
- Tests for `src/compiler/` and `src/event_loop.rs` integration belong in `tests/parity.rs` or a new integration test.

## Acceptance criteria

- `crates/quench-runtime/tests/` exists and `cargo test -p quench-runtime` runs all tests.
- Every Rank 1 and Rank 2 finding from Task 26 has at least one regression test.
- New features added after this task include tests in the same commit.

## Verification

```bash
cargo test -p quench-runtime
cargo test
```
