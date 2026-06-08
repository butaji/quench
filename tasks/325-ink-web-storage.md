# Task 325: `ink-web-storage` Example — `localStorage` / `sessionStorage`

**Priority:** P2-Medium
**Phase:** 26 — Browser Globals
**Depends on:** 324

## Problem

`localStorage` and `sessionStorage` provide key-value storage in browsers. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-web-storage/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  localStorage.setItem('key', 'value');
  const value = localStorage.getItem('key');
  const keys = Object.keys(localStorage);

  return (
    <Box flexDirection="column">
      <Text>Value: {value}</Text>
      <Text>Keys: {keys.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-web-storage/`
- [ ] Uses `localStorage` / `sessionStorage` detection
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
