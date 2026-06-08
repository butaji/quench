# Task 374: `ink-timers-promises` Example — `timers/promises` (`setTimeout` / `setInterval` as promises)

**Priority:** P2-Medium
**Phase:** 29 — Node.js Standard Library
**Depends on:** 373

## Problem

`timers/promises` provides promise-based timer APIs (`setTimeout`, `setInterval`). No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-timers-promises/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [ready, setReady] = useState(false);

  useEffect(() => {
    const delay = (ms: number) => new Promise(r => setTimeout(r, ms));
    delay(50).then(() => setReady(true));
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Ready: {String(ready)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-timers-promises/`
- [ ] Uses promise-based timer patterns
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
