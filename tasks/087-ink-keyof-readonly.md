# Task 087: `ink-keyof-readonly` Example — `keyof`, `readonly` Arrays/Tuples

**Priority:** P1-High
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

`keyof` operator and `readonly` modifier for arrays/tuples are common TypeScript utilities. No existing Ink example exercises these in a TUI context.

## Ink Example

```tsx
// examples/ink-keyof-readonly/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

interface Settings {
  theme: string;
  width: number;
  height: number;
}

type SettingKey = keyof Settings;

const settings: Settings = { theme: 'dark', width: 80, height: 24 };
const keys: readonly SettingKey[] = ['theme', 'width', 'height'];
const tuple: readonly [string, number] = ['hello', 42];

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Keys: {keys.join(', ')}</Text>
      <Text>Tuple: {tuple[0]} {tuple[1]}</Text>
      <Text>Theme: {settings.theme}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-keyof-readonly/`
- [ ] Uses `keyof` operator
- [ ] Uses `readonly` arrays and tuples
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases `keyof` and `readonly` without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments