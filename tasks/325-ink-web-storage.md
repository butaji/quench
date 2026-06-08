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
  const hasStorage = typeof localStorage !== 'undefined';

  return (
    <Box flexDirection="column">
      <Text>Has localStorage: {String(hasStorage)}</Text>
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
