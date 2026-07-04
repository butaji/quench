# Task 290: Complete expression syntax gaps

## Status: PENDING

## Gap

Several modern expression forms are not lowered/executed: template literals with embedded expressions, computed object keys, spread in arrays/objects/calls, nullish coalescing (`??`), optional chaining (`?.`), `delete`, unary `+`, and `for-of` loops. These show up as top TS expression failures.

## Fix

Extend `lower.rs` and the AST/interpreter to support each form. Optional chaining is partially present but may need completion.

## Acceptance criteria

- [ ] Template literal: `` `a${1+2}b` `` evaluates to `"a3b"`.
- [ ] Computed key: `{ [k]: v }` works.
- [ ] Spread: `[...arr]`, `{...obj}`, `fn(...args)` work.
- [ ] Nullish coalescing: `a ?? b` short-circuits on null/undefined.
- [ ] Optional chaining: `a?.b?.c` works.
- [ ] `delete obj.prop` returns true/false.
- [ ] Unary `+` coerces to number.
- [ ] `for-of` iterates arrays/strings.
- [ ] One focused test per syntax form plus JS scenario tests.

## Files

- `crates/quench-runtime/src/lower.rs`
- `crates/quench-runtime/src/ast.rs`
- `crates/quench-runtime/src/interpreter/*.rs`

## Tests unblocked

- 100+ TypeScript expression failures
- Many test262 expression/statement suites
