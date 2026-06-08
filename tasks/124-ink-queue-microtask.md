# Task 124: `ink-queue-microtask` Example — `queueMicrotask`

**Priority:** P1-High
**Phase:** 12 — Runtime API Completion
**Depends on:** 123

## Problem

`queueMicrotask` is the standard way to schedule a microtask in JavaScript. It's commonly used for deferring work to the end of the current event loop iteration. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-queue-microtask/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [order, setOrder] = useState<string[]>([]);

  useEffect(() => {
    setOrder(prev => [...prev, 'sync']);
    queueMicrotask(() => {
      setOrder(prev => [...prev, 'microtask']);
    });
    Promise.resolve().then(() => {
      setOrder(prev => [...prev, 'promise']);
    });
    setTimeout(() => {
      setOrder(prev => [...prev, 'timeout']);
    }, 0);
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Order: {order.join(' < ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-queue-microtask/`
- [ ] Uses `queueMicrotask` to schedule microtask
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
