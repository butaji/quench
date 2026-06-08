# Task 186: `ink-symbol-async-iterator` Example — `Symbol.asyncIterator`

**Priority:** P1-High
**Phase:** 17 — ES2018 Features
**Depends on:** 185

## Problem

`Symbol.asyncIterator` is the protocol for async iterables. Task 155 covers `Symbol.iterator` but not the async variant. No existing Ink example exercises `Symbol.asyncIterator`.

## Ink Example

```tsx
// examples/ink-symbol-async-iterator/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

class AsyncCounter {
  private count = 0;

  async *[Symbol.asyncIterator]() {
    while (this.count < 5) {
      await new Promise(r => setTimeout(r, 10));
      yield ++this.count;
    }
  }
}

export default function App() {
  const [items, setItems] = useState<number[]>([]);

  useEffect(() => {
    const collect = async () => {
      const result: number[] = [];
      const counter = new AsyncCounter();
      for await (const n of counter) {
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

- [ ] Example exists at `examples/ink-symbol-async-iterator/`
- [ ] Uses `*[Symbol.asyncIterator]()` async generator
- [ ] Uses `for await...of` with async iterable
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
