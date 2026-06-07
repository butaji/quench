# Task 101: `ink-as-const` Example — `as const`, Literal Types, Tuple Types

**Priority:** P1-High
**Phase:** 11 — Type System Deep Coverage
**Depends on:** 078

## Problem

`as const` assertions, literal types, and tuple types are fundamental TypeScript patterns for creating immutable values and precise type constraints. No existing Ink example exercises these.

## Ink Example

```tsx
// examples/ink-as-const/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const COLORS = ['red', 'green', 'blue'] as const;
type Color = typeof COLORS[number];

const CONFIG = {
  title: 'My App',
  version: 1,
  features: ['auth', 'billing'],
} as const;

type Config = typeof CONFIG;

type Point = [x: number, y: number];
const origin: Point = [0, 0];

const STATUS = ['idle', 'loading', 'done'] as const;
type Status = typeof STATUS[number];
const status: Status = 'idle';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Title: {CONFIG.title}</Text>
      <Text>Version: {CONFIG.version}</Text>
      <Text>Features: {CONFIG.features.join(', ')}</Text>
      <Text>Origin: ({origin[0]}, {origin[1]})</Text>
      <Text>Status: {status}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-as-const/`
- [ ] Uses `as const` assertion
- [ ] Uses literal types (`typeof COLORS[number]`)
- [ ] Uses labeled tuple types (`[x: number, y: number]`)
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases `as const` and literal types without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
