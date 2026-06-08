# Task 141: `ink-namespace-reexport` Example — `export * as ns from "mod"`

**Priority:** P1-High
**Phase:** 12 — Module Pattern Completion
**Depends on:** 086

## Problem

`export * as ns from "mod"` (namespace re-export) is a common pattern for creating module facades. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-namespace-reexport/utils/math.ts
export const add = (a: number, b: number): number => a + b;
export const mul = (a: number, b: number): number => a * b;

// examples/ink-namespace-reexport/utils/index.ts
export * as Math from './math.js';

// examples/ink-namespace-reexport/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';
import { Math } from '../utils/index.js';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>2 + 3 = {Math.add(2, 3)}</Text>
      <Text>4 * 5 = {Math.mul(4, 5)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-namespace-reexport/`
- [ ] Uses `export * as ns from './module'` pattern
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles namespace re-exports
- [ ] Parity harness passes with 100% match in all 3 environments
