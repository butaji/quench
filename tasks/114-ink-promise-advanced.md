# Task 114: `ink-promise-advanced` Example — `allSettled`, `any`, `race`, `withResolvers`

**Priority:** P2-Medium
**Phase:** 11 — Runtime API Coverage
**Depends on:** 078

## Problem

Advanced Promise methods (`allSettled`, `any`, `race`, `withResolvers`) are essential for robust async handling. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-promise-advanced/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [results, setResults] = useState<string[]>([]);

  useEffect(() => {
    Promise.allSettled([
      Promise.resolve('ok'),
      Promise.reject('err'),
    ]).then((r) => {
      setResults(r.map(x => x.status));
    });

    Promise.race([
      new Promise(r => setTimeout(() => r('fast'), 10)),
      new Promise(r => setTimeout(() => r('slow'), 100)),
    ]).then(winner => {
      setResults(prev => [...prev, String(winner)]);
    });

    // Promise.withResolvers is ES2024 - optional
    if ('withResolvers' in Promise) {
      const { resolve } = Promise.withResolvers();
      resolve('resolved');
    }
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Results: {results.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-promise-advanced/`
- [ ] Uses `Promise.allSettled`, `Promise.race`
- [ ] Optionally uses `Promise.withResolvers`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
