# Task 346: `ink-non-null-chain` Example — Non-Null Assertion After Optional Chain

**Priority:** P1-High
**Phase:** 27 — TypeScript Syntax
**Depends on:** 345

## Problem

Non-null assertion after optional chain (`obj?.field!.method()`) combines optional chaining with type assertions. No existing Ink example exercises this pattern.

## Ink Example

```tsx
// examples/ink-non-null-chain/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

interface Item {
  nested: { value: string };
}

export default function App() {
  const item: Item | undefined = { nested: { value: 'found' } };
  const value = item?.nested!.value;

  return (
    <Box flexDirection="column">
      <Text>Value: {value}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-non-null-chain/`
- [ ] Uses `!.` after optional chain
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases non-null assertion without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
