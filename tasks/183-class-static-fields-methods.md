> **Part of Task 182 class support.** Implement after Task 182 core class support lands.

# Task 183: Implement class static fields and methods

## Status: PENDING

## Goal

Support `static foo = 1` and `static bar() {}` in class declarations and expressions.

## Exact implementation

1. In `lower_class_member` (Task 182), add a `ClassMember::Field` variant for static fields with `is_static: true`.
2. In `eval_class`, after creating the constructor function:
   - For each `ClassMember::Field { is_static: true, name, init }`, define the property on the constructor function object with the evaluated initializer (or `undefined`).
   - For each `ClassMember::Method { is_static: true, ... }`, define the method on the constructor function object.
3. Use the same property-definition helper as instance members, but target the constructor instead of `prototype`.

## Acceptance criteria

- [ ] `class A { static x = 1; }` sets `A.x === 1`.
- [ ] `class A { static f() { return 2; } }` sets `A.f() === 2`.
- [ ] Static fields are initialized at class-definition time, not per-instance.
- [ ] Regression tests and spec fixtures added.

## Targets

- **Suite:** `both`
- **Batch:** 4
- **Target subset:** `tests/test262/test/language/class/static/` + TypeScript static member cases
- **Blocked by:** 182
- **Exit criteria:** Static field/method subsets pass at 100% with zero spec skips.
