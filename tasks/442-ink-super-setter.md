# Task 442: `ink-super-setter` Example — `super.prop = value` in Setter Context

**Priority:** P1-High
**Phase:** 34 — HIR & Codegen Edge Cases
**Depends on:** 441

## Problem

`super` in a setter assignment (`super.prop = value`) is distinct from `super.method()` and `super()` because it is an assignment expression with a `Super` base. Task 222 covers `super()` in constructors, Task 407 covers `super.method()`, but `super.prop = value` in setters is not explicitly exercised.

## HIR Coverage

- `Expr::Assign` with `Expr::Member` left-hand side where base is `Super`
- Setter definitions in derived classes

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for assignment expression evaluation
- Generated code must correctly route to superclass setter

## Ink Example

```tsx
// examples/ink-super-setter/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

class Base {
  private _value = 0;

  get value(): number {
    return this._value;
  }

  set value(v: number) {
    this._value = v;
  }
}

class Derived extends Base {
  set value(v: number) {
    super.value = v * 2;
  }
}

const d = new Derived();
d.value = 5;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Value: {d.value}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-super-setter/`
- [ ] Uses `super.prop = value` in a derived class setter
- [ ] HIR `Expr::Assign` with `Super` base produces compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
