> Sub-task of 290: complete expression syntax gaps.

# Task 290b: Implement computed property keys

## Status: PENDING

## Gap

Computed object literal keys (`{ [k]: v }`) are not supported. Object literals only accept static identifiers and string literals.

## Fix

- Lower computed property keys (`swc::PropName::Computed`) to `PropertyKey::Computed`.
- Evaluate the key expression at object-creation time and coerce it to a property key.

## Acceptance criteria

- [ ] `{ ["a" + "b"]: 1 }` produces `{ ab: 1 }`.
- [ ] `{ [Symbol.iterator]: fn }` works when `Symbol` is available.
- [ ] Duplicate computed keys follow spec semantics (last wins in non-strict).
- [ ] Regression test and JS scenario test.

## Files

- `crates/quench-runtime/src/lower.rs`
- `crates/quench-runtime/src/ast.rs`
- `crates/quench-runtime/src/interpreter/eval_expr.rs`

## Tests unblocked

- test262 `language/expressions/object/computed-property-names/`
- TypeScript object literal failures
