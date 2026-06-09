# Task 354: `ink-satisfies-union` Example — `satisfies` with Union Types

**Priority:** P1-High
**Phase:** 28 — TypeScript 4.9+ Features
**Depends on:** 353

## Problem

`satisfies` with union types (`const x = value satisfies string | number`) constrains a value to a union while preserving its literal type. No existing Ink example exercises this pattern.

## Ink Example

```tsx
// examples/ink-satisfies-union/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const status = 'active' satisfies 'active' | 'inactive';
const count = 5 satisfies number;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Status: {status}</Text>
      <Text>Count: {count}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Type erasure (no runtime HIR needed)

## Compile-Path Codegen

- Type erasure at parse time (no runtime codegen)

## Acceptance Criteria

- [ ] Example exists at `examples/ink-satisfies-union/`
- [ ] Uses `satisfies` with union type
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases `satisfies` without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
