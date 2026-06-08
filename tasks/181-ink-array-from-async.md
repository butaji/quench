# Task 181: `ink-array-from-async` Example — `Array.fromAsync`

**Priority:** P1-High
**Phase:** 17 — ES2024 Features
**Depends on:** 180

## Problem

`Array.fromAsync` (ES2024) creates an array from an async iterable or async iterator. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-array-from-async/tui/app.tsx
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
    Array.fromAsync(asyncRange(1, 5)).then(result => {
      setItems(result);
    });
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Items: {items.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-array-from-async/`
- [ ] Uses `Array.fromAsync` with async iterable
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
