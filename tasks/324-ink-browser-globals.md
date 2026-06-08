# Task 324: `ink-browser-globals` Example — `window`, `document`, `navigator`, `location`

**Priority:** P2-Medium
**Phase:** 26 — Browser Globals
**Depends on:** 323

## Problem

Browser globals (`window`, `document`, `navigator`, `location`, `history`) may appear in isomorphic or universal code. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-browser-globals/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const hasWindow = typeof window !== 'undefined';
  const hasDocument = typeof document !== 'undefined';
  const hasNavigator = typeof navigator !== 'undefined';

  return (
    <Box flexDirection="column">
      <Text>Has window: {String(hasWindow)}</Text>
      <Text>Has document: {String(hasDocument)}</Text>
      <Text>Has navigator: {String(hasNavigator)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-browser-globals/`
- [ ] Uses `window`, `document`, `navigator` detection
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for browser globals
- [ ] Parity harness passes with 100% match in all 3 environments
