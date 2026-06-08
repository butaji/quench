# Task 294: `ink-satisfies-function` Example — `satisfies` on Function Expressions

**Priority:** P2-Medium
**Phase:** 24 — TypeScript 4.9+ Features
**Depends on:** 293

## Problem

`satisfies` can be applied to function expressions to constrain their inferred type without changing runtime behavior. No existing example explicitly exercises this pattern.

## Ink Example

```tsx
// examples/ink-satisfies-function/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

type Adder = (a: number, b: number) => number;

const add = ((a, b) => a + b) satisfies Adder;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Sum: {add(2, 3)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-satisfies-function/`
- [ ] Uses `satisfies` on a function expression
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases `satisfies` without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
