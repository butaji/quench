# Task 306: `ink-abort-signal` Example — `AbortSignal` and `AbortController`

**Priority:** P1-High
**Phase:** 25 — Web APIs
**Depends on:** 305

## Problem

`AbortSignal` and `AbortController` provide a standard way to cancel async operations. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-abort-signal/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [aborted, setAborted] = useState(false);

  useEffect(() => {
    const controller = new AbortController();
    const signal = controller.signal;

    setTimeout(() => {
      controller.abort();
      setAborted(signal.aborted);
    }, 50);

    return () => controller.abort();
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Aborted: {String(aborted)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-abort-signal/`
- [ ] Uses `AbortController` and `AbortSignal`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
