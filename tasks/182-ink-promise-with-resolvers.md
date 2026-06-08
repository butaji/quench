# Task 182: `ink-promise-with-resolvers` Example — `Promise.withResolvers`

**Priority:** P1-High
**Phase:** 17 — ES2024 Features
**Depends on:** 181

## Problem

`Promise.withResolvers` (ES2024) creates a Promise with `resolve` and `reject` functions exposed externally. Task 114 covers `Promise.allSettled`/`any`/`race` but not `withResolvers`. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-promise-with-resolvers/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [status, setStatus] = useState('pending');

  useEffect(() => {
    const { promise, resolve } = Promise.withResolvers();

    setTimeout(() => {
      resolve('done');
    }, 100);

    promise.then((v: string) => {
      setStatus(v);
    });
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Status: {status}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-promise-with-resolvers/`
- [ ] Uses `Promise.withResolvers()`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
