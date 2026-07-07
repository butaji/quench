> **Minimum custom code.** Use the standard ECMAScript object model instead of custom variants.

# Task 335: Collapse value model into Value::Object with [[Call]] / [[Construct]] slots

## Goal

Replace the separate `Value::Function`, `Value::NativeFunction`, `Value::NativeConstructor`, `Value::Array`, etc. variants with a single `Value::Object` whose internal object record carries `[[Call]]` and `[[Construct]]` slots when applicable.

## Rationale

The current value model has custom branches for every callable/constructible kind. ECMAScript itself models functions, constructors, and built-ins as ordinary objects with internal slots. Collapsing to one representation removes duplicate code paths and makes the spec semantics fall out naturally.

## Acceptance criteria

- [ ] `Value` no longer has separate function/constructor/array variants.
- [ ] Every callable value is an object with a `[[Call]]` internal method.
- [ ] Every constructible value is an object with a `[[Construct]]` internal method.
- [ ] Existing tests pass without regressions.

## Targets

- **Suite:** `both`
- **Batch:** 1
- **Target subset:** test262 + TypeScript function/constructor/object subsets
- **Blocked by:** 85, 322
- **Exit criteria:** Value model is unified and relevant function/constructor/object subsets reach 100%.
