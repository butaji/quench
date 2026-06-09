# Task 428: `ink-class-expression-extends` Example — Anonymous Class Expression with `extends` and `super`

**Priority:** P1-High
**Phase:** 34 — HIR & Codegen Edge Cases
**Depends on:** 427

## Problem

Anonymous class expressions with `extends` (`const Derived = class extends Base { ... }`) exercise HIR class expression handling with superclass references and `super()` calls. Task 191 covers anonymous class expressions without inheritance. No example covers the combination.

## HIR Coverage

- `Expr::Class` with a superclass expression
- `Expr::SuperCall` inside anonymous class constructor
- `Expr::Member` with `Super` base for `super.method()`

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for class expression evaluation
- `quote_codegen.rs` for class declaration with inheritance codegen

## Ink Example

```tsx
// examples/ink-class-expression-extends/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const Base = class {
  name = 'Base';
  greet(): string {
    return `Hello from ${this.name}`;
  }
};

const Derived = class extends Base {
  constructor() {
    super();
    this.name = 'Derived';
  }
  greet(): string {
    return super.greet() + '!';
  }
};

const d = new Derived();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>{d.greet()}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-class-expression-extends/`
- [ ] Uses anonymous class expression with `extends` and `super()`
- [ ] HIR `Expr::Class` with superclass produces compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
