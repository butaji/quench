# Task 348: `ink-nullish-jsx-attr` Example — Nullish Coalescing in JSX Attributes

**Priority:** P1-High
**Phase:** 27 — JSX Expression Patterns
**Depends on:** 347

## Problem

Nullish coalescing inside JSX attributes (`color={userColor ?? 'green'}`) is a common pattern for defaults. No dedicated Ink example exercises this.

## Ink Example

```tsx
// examples/ink-nullish-jsx-attr/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const userColor: string | undefined = undefined;
  const userName: string | null = null;

  return (
    <Box flexDirection="column">
      <Text color={userColor ?? 'green'}>Default green</Text>
      <Text>{userName ?? 'Anonymous'}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-nullish-jsx-attr/`
- [ ] Uses `??` in JSX attribute and children
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
