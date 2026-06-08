# Task 241: `ink-keyof-typeof` Example — `keyof typeof` Pattern

**Priority:** P1-High
**Phase:** 21 — TypeScript Type Patterns
**Depends on:** 240

## Problem

The `keyof typeof` pattern (`type Keys = keyof typeof obj`) extracts string literal union keys from an object. No existing Ink example explicitly exercises this pattern.

## Ink Example

```tsx
// examples/ink-keyof-typeof/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const themes = {
  light: 'bg-white',
  dark: 'bg-black',
  auto: 'bg-system',
} as const;

type ThemeKey = keyof typeof themes;

export default function App() {
  const selected: ThemeKey = 'dark';

  return (
    <Box flexDirection="column">
      <Text>Selected: {selected}</Text>
      <Text>Value: {themes[selected]}</Text>
      <Text>Keys: {(Object.keys(themes) as ThemeKey[]).join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-keyof-typeof/`
- [ ] Uses `keyof typeof` on a const object
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases `keyof typeof` without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
