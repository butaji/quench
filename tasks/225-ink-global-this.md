# Task 225: `ink-global-this` Example — `globalThis`

**Priority:** P1-High
**Phase:** 20 — Advanced Language Features
**Depends on:** 224

## Problem

`globalThis` is the standard way to access the global object across environments. Task 088 covers `bigint-globalthis` but `globalThis` is only a secondary feature there. No dedicated example exists.

## Ink Example

```tsx
// examples/ink-global-this/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const hasConsole = 'console' in globalThis;
  const hasProcess = 'process' in globalThis;
  const globalType = typeof globalThis;

  return (
    <Box flexDirection="column">
      <Text>Has console: {String(hasConsole)}</Text>
      <Text>Has process: {String(hasProcess)}</Text>
      <Text>Type: {globalType}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-global-this/`
- [ ] Uses `globalThis` to access global properties
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for `globalThis`
- [ ] Parity harness passes with 100% match in all 3 environments
