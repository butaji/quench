# Task 165: `ink-for-await-of` Example — `for await...of` Async Iteration

**Priority:** P1-High
**Phase:** 16 — Async Feature Completion
**Depends on:** 052

## Problem

`for await...of` loops iterate over async iterables. This is a core async pattern not yet covered by any Ink example.

## Ink Example

```tsx
// examples/ink-for-await-of/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

async function* asyncRange(start: number, end: number) {
  for (let i = start; i <= end; i++) {
    await new Promise(r => setTimeout(r, 10));
    yield i;
  }
}

export default function App() {
  const [items, setItems] = useState<number[]>([]);

  useEffect(() => {
    const collect = async () => {
      const result: number[] = [];
      for await (const n of asyncRange(1, 5)) {
        result.push(n);
      }
      setItems(result);
    };
    collect();
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Items: {items.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-for-await-of/`
- [ ] Uses `for await...of` with async generator
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
