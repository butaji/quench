# Task 309: `ink-set-immediate` Example — `setImmediate` / `clearImmediate`

**Priority:** P2-Medium
**Phase:** 25 — Node.js Timers
**Depends on:** 308

## Problem

`setImmediate` and `clearImmediate` schedule callbacks to run after I/O events. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-set-immediate/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [count, setCount] = useState(0);

  useEffect(() => {
    const id = setImmediate(() => {
      setCount(c => c + 1);
    });
    return () => clearImmediate(id);
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Count: {count}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-set-immediate/`
- [ ] Uses `setImmediate` and `clearImmediate`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
