# Task 295: Use global object as environment record

## Status: PENDING

## Gap

`var x = 1` does not create `globalThis.x`, and `globalThis.y = 1` does not make bare `y` resolvable. The global object and top-level environment are disconnected.

## Fix

Use the global object as the root environment record (or mirror var/global-property assignments).

## Acceptance criteria

- [ ] `var x = 1; globalThis.x === 1` is true.
- [ ] `globalThis.y = 2; y === 2` is true.
- [ ] Regression test and JS scenario test.

## Files

- `crates/quench-runtime/src/lib.rs`
- `crates/quench-runtime/src/interpreter/core.rs`
- `crates/quench-runtime/src/value.rs`

## Tests unblocked

- Global-object semantics tests
- TS baselines that pollute `globalThis`
