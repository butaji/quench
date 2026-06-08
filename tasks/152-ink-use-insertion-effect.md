# Task 152: `ink-use-insertion-effect` Example — `useInsertionEffect`

**Priority:** P1-High
**Phase:** 14 — React 18 Hook Coverage
**Depends on:** 080

## Problem

`useInsertionEffect` (React 18) is a hook for injecting styles before DOM mutations. It's the last major React 18 hook not yet covered by any Ink example.

## Ink Example

```tsx
// examples/ink-use-insertion-effect/tui/app.tsx
import React, { useInsertionEffect, useState } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [ready, setReady] = useState(false);

  useInsertionEffect(() => {
    setReady(true);
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Insertion effect: {ready ? 'ready' : 'pending'}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-use-insertion-effect/`
- [ ] Uses `useInsertionEffect` hook
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments