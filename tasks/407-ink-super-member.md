# Task 407: `ink-super-member` Example — `super.method()` and `super[prop]()` Access

**Priority:** P1-High
**Phase:** 33 — HIR & Codegen Edge Cases
**Depends on:** 406

## Problem

`super()` in constructors is covered by Task 222, but `super.method()` and `super[expr]()` in derived class methods are distinct HIR constructs that exercise `Super` base expressions in member access. No existing Ink example explicitly exercises `super` member access.

## HIR Coverage

- `Expr::Member` with a `Super` base object (distinct from `Expr::SuperCall`)
- `Expr::ComputedMember` with a `Super` base object
- Method definitions in derived classes that reference `super`

## Compile-Path Codegen

- `quote_codegen_exprs.inc` must emit compilable Rust for `super.method()` and `super[expr]()`
- Generated code must correctly resolve the superclass method in the Rust inheritance model

## Ink Example

```tsx
// examples/ink-super-member/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

class Base {
  greet(): string {
    return 'Hello from Base';
  }

  getName(): string {
    return 'Base';
  }
}

class Derived extends Base {
  greet(): string {
    return super.greet() + ' via Derived';
  }

  getName(): string {
    const method = 'getName';
    return super[method]() as string;
  }
}

const d = new Derived();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>{d.greet()}</Text>
      <Text>{d.getName()}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-super-member/`
- [ ] Uses `super.method()` direct member access
- [ ] Uses `super[expr]()` computed member access
- [ ] HIR `Expr::Member` / `Expr::ComputedMember` with `Super` base produces compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
