# Task 355: `ink-as-const-function` Example — `as const` in Function Returns

**Priority:** P1-High
**Phase:** 28 — TypeScript Type Patterns
**Depends on:** 354

## Problem

`as const` in function returns (`return [x, y] as const`) produces readonly tuple literal types. No existing Ink example exercises this pattern.

## Ink Example

```tsx
// examples/ink-as-const-function/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function getPoint() {
  return [10, 20] as const;
}

function getConfig() {
  return { theme: 'dark', width: 80 } as const;
}

export default function App() {
  const [x, y] = getPoint();
  const config = getConfig();

  return (
    <Box flexDirection="column">
      <Text>Point: {x}, {y}</Text>
      <Text>Theme: {config.theme}</Text>
      <Text>Width: {config.width}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-as-const-function/`
- [ ] Uses `as const` in function return
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases `as const` without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
