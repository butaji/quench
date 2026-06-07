# Task 098: `ink-infer-conditional` Example — `infer` in Conditional Types

**Priority:** P3-Low
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

`infer` keyword in conditional types (TS 2.8) enables type extraction from complex structures. It is purely type-level and erased at compile time. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-infer-conditional/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

type ReturnType<T> = T extends (...args: any[]) => infer R ? R : never;

function getName(): string {
  return 'App';
}

type NameType = ReturnType<typeof getName>;
const name: NameType = 'Test';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Name: {name}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-infer-conditional/`
- [ ] Uses `infer` keyword in conditional type
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases `infer` without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments