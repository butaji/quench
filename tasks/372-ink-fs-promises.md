# Task 372: `ink-fs-promises` Example — `fs/promises` Module

**Priority:** P2-Medium
**Phase:** 29 — Node.js Standard Library
**Depends on:** 371

## Problem

`fs/promises` provides promise-based file system APIs. Task 315 covers `fs`; no example covers the promise-based variant.

## Ink Example

```tsx
// examples/ink-fs-promises/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>fs/promises example</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-fs-promises/`
- [ ] References `fs/promises` API patterns
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
