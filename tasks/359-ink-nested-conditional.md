# Task 359: `ink-nested-conditional` Example — Nested Conditional Types

**Priority:** P2-Medium
**Phase:** 28 — Advanced Type System Patterns
**Depends on:** 358

## Problem

Nested conditional types (`T extends A ? (T extends B ? X : Y) : Z`) enable complex type-level branching. No existing Ink example exercises deeply nested conditionals.

## Ink Example

```tsx
// examples/ink-nested-conditional/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

type Flatten<T> = T extends Array<infer U> ? (U extends Array<infer V> ? V : U) : T;

const flat: Flatten<string[][]> = 'deeply-flattened';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Value: {flat}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Type erasure (no runtime HIR needed)

## Compile-Path Codegen

- Type erasure at parse time (no runtime codegen)

## Acceptance Criteria

- [ ] Example exists at `examples/ink-nested-conditional/`
- [ ] Uses nested conditional type with multiple `infer`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases conditional types without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
