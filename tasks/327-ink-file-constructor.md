# Task 327: `ink-file-constructor` Example — `File` Constructor

**Priority:** P2-Medium
**Phase:** 26 — Web APIs
**Depends on:** 326

## Problem

The `File` constructor extends `Blob` with name and last-modified properties. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-file-constructor/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const hasFile = typeof File !== 'undefined';
  const info = hasFile
    ? { name: 'test.txt', size: 12, type: 'text/plain' }
    : { name: 'n/a', size: 0, type: 'n/a' };

  return (
    <Box flexDirection="column">
      <Text>Has File: {String(hasFile)}</Text>
      <Text>Name: {info.name}</Text>
      <Text>Size: {info.size}</Text>
      <Text>Type: {info.type}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-file-constructor/`
- [ ] Uses `File` constructor or documents availability
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
