# Task 313: `ink-os-module` Example — `os` Module

**Priority:** P2-Medium
**Phase:** 25 — Node.js Runtime APIs
**Depends on:** 312

## Problem

The `os` module provides operating system-related utilities. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-os-module/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const platform = typeof process !== 'undefined' ? process.platform : 'unknown';
  const arch = typeof process !== 'undefined' ? process.arch : 'unknown';

  return (
    <Box flexDirection="column">
      <Text>Platform: {platform}</Text>
      <Text>Arch: {arch}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-os-module/`
- [ ] Uses `process.platform` or `process.arch`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
