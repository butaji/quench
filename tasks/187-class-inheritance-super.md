> **Part of Task 182 class support.** Implement together with Task 182.

# Task 187: Implement class inheritance and super

## Status: PENDING

## Goal

Support `class B extends A`, `super()`, `super.method()`, and static inheritance.

## Exact implementation

1. In `lower_class`, capture `super_class: Some(Box::new(lower_expr(&class.super_class)?))` when `extends` is present.
2. In `eval_class`:
   - Resolve the superclass expression to a function object.
   - Create the subclass constructor that calls `super(...)` when invoked.
   - Set the subclass constructor's `[[Prototype]]` to the superclass (for static inheritance).
   - Set `prototype.[[Prototype]]` to `superclass.prototype`.
3. In the interpreter call path for class constructors:
   - Before executing the subclass body, bind `this` from `super()`.
   - If the subclass constructor does not explicitly call `super()`, throw a `ReferenceError` when `this` is accessed.
4. For `super.method()`:
   - Resolve `super` as the superclass prototype.
   - Resolve `this` as the current instance.
   - Call the method with `this` bound to the instance.

## Acceptance criteria

- [ ] `class B extends A {}` makes `new B() instanceof A === true`.
- [ ] `super()` in a subclass constructor initializes `this`.
- [ ] `super.method()` calls the superclass method with the current `this`.
- [ ] Static members inherit from superclass.
- [ ] Regression tests and spec fixtures added.

## Targets

- **Suite:** `both`
- **Batch:** 4
- **Target subset:** `tests/test262/test/language/class/extends/` + TypeScript inheritance cases
- **Blocked by:** 182
- **Exit criteria:** Inheritance/`super` subsets pass at 100% with zero spec skips.
