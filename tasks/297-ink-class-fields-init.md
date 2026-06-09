# Task 297: `ink-class-fields-init` Example — Class Fields with Complex Initializers

**Priority:** P1-High
**Phase:** 24 — Class Features
**Depends on:** 296

## Problem

Class fields with complex initializers (`x = this.computeX()`) exercise class field evaluation order. No dedicated example exercises this pattern.

## Ink Example

```tsx
// examples/ink-class-fields-init/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

class Example {
  base = 10;
  doubled = this.base * 2;
  greeting = `Value is ${this.doubled}`;

  compute(): string {
    return this.greeting;
  }
}

const ex = new Example();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>{ex.compute()}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `ClassMember` and `Class` variants

## Compile-Path Codegen

- `quote_codegen.rs` for class declaration codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-class-fields-init/`
- [ ] Uses class fields initialized from other fields/expressions
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for class field initializers
- [ ] Parity harness passes with 100% match in all 3 environments
