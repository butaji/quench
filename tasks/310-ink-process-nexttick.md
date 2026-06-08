# Task 310: `ink-process-nexttick` Example — `process.nextTick`

**Priority:** P2-Medium
**Phase:** 25 — Node.js Runtime APIs
**Depends on:** 309

## Problem

`process.nextTick` schedules a callback before the next event loop iteration. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-process-nexttick/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [order, setOrder] = useState<string[]>([]);

  useEffect(() => {
    const seq: string[] = [];
    seq.push('sync');
    setTimeout(() => { seq.push('timeout'); }, 0);
    process.nextTick(() => { seq.push('nextTick'); setOrder([...seq]); });
    seq.push('after-nextTick');
    setOrder([...seq]);
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Order: {order.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-process-nexttick/`
- [ ] Uses `process.nextTick`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for process.nextTick
- [ ] Parity harness passes with 100% match in all 3 environments
