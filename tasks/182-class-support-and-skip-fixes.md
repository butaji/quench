# Task 182: Class support and conformance skip fixes

## Status: PENDING

## Gap

JavaScript class syntax (`class`, `extends`, `super`, `static`) is not implemented or is stripped too early. Class-related test262 and TypeScript conformance cases are skipped or fail.

## Fix

- Lower `class` declarations and expressions to runtime constructs.
- Implement `extends`, `super()` calls, `super.method`, static methods, and static fields.
- Wire `constructor` as the class constructor and set up the prototype chain.

## Acceptance criteria

- [ ] `class A {}` creates a constructor function with correct prototype.
- [ ] `class B extends A {}` sets up inheritance and `instanceof` works.
- [ ] `super()` and `super.method()` work inside class methods.
- [ ] Static methods and fields are attached to the constructor.
- [ ] Regression tests and JS scenario tests for class behavior.

## Files

- `crates/quench-runtime/src/lower.rs`
- `crates/quench-runtime/src/ast.rs`
- `crates/quench-runtime/src/interpreter/*.rs`
- `crates/quench-runtime/src/value.rs`

## Tests unblocked

- test262 `language/class/`
- TypeScript class conformance cases
