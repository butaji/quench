# Task 275: `ink-async-generator` Example — Async Generators

**Priority:** P1-High
**Phase:** 23 — Advanced Language Features
**Depends on:** 274

## Problem

Async generators (`async function*`) combine async functions with generators. Task 053 covers sync generators; no existing example exercises async generators.

## Ink Example

```tsx
// examples/ink-async-generator/tui/app.tsx
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

- [ ] Example exists at `examples/ink-async-generator/`
- [ ] Uses `async function*` declaration
- [ ] Uses `for await...of` with async generator
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
