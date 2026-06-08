# Task 347: `ink-type-assertion-jsx` Example — Type Assertions in JSX Expressions

**Priority:** P1-High
**Phase:** 27 — JSX Type Patterns
**Depends on:** 346

## Problem

Type assertions inside JSX expressions (`{value as string}`) are common for narrowing dynamic values. No existing Ink example exercises this pattern.

## Ink Example

```tsx
// examples/ink-type-assertion-jsx/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const value: unknown = 'typed value';

  return (
    <Box flexDirection="column">
      <Text>{value as string}</Text>
      <Text>{(value as string).toUpperCase()}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-type-assertion-jsx/`
- [ ] Uses `as` type assertion inside JSX expression
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases type assertion without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
