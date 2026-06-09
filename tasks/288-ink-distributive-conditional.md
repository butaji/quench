# Task 288: `ink-distributive-conditional` Example — Distributive Conditional Types

**Priority:** P2-Medium
**Phase:** 24 — Type System Deep Coverage
**Depends on:** 287

## Problem

Distributive conditional types (`type Filter<T, U> = T extends U ? T : never`) operate on union types member-by-member. No existing Ink example exercises this pattern.

## Ink Example

```tsx
// examples/ink-distributive-conditional/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

type FilterString<T> = T extends string ? T : never;
type Mixed = string | number | boolean;
type OnlyStrings = FilterString<Mixed>;

const value: OnlyStrings = 'hello';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Value: {value}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Type erasure (no runtime HIR needed)

## Compile-Path Codegen

- Type erasure at parse time (no runtime codegen)

## Acceptance Criteria

- [ ] Example exists at `examples/ink-distributive-conditional/`
- [ ] Uses distributive conditional type over a union
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases conditional types without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
