# Task 184: `ink-const-type-param` Example — `const T` Type Parameters (TS 5.0)

**Priority:** P1-High
**Phase:** 17 — TypeScript 5.0+ Features
**Depends on:** 183

## Problem

`const` type parameters (`function fn<const T>(x: T)`) are a TypeScript 5.0 feature that infers tuple and literal types instead of widening. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-const-type-param/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function createTuple<const T extends readonly unknown[]>(...args: T): T {
  return args;
}

const tuple = createTuple('a', 1, true);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Tuple: {tuple.join(', ')}</Text>
      <Text>Types inferred as literal</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-const-type-param/`
- [ ] Uses `function fn<const T>()` syntax
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases `const` type parameter without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
