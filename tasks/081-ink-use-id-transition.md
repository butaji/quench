# Task 081: `ink-use-id-transition` Example — `useId`, `useTransition`

**Priority:** P1-High
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

React 18 hooks `useId` (stable IDs for accessibility) and `useTransition` (non-blocking state updates) are not exercised by any existing Ink example. These are important for real-world apps.

## Ink Example

```tsx
// examples/ink-use-id-transition/tui/app.tsx
import React, { useId, useState, useTransition } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [isPending, startTransition] = useTransition();
  const [count, setCount] = useState(0);
  const id = useId();

  const increment = () => {
    startTransition(() => {
      setCount(c => c + 1);
    });
  };

  return (
    <Box flexDirection="column">
      <Text>ID: {id}</Text>
      <Text>Count: {count}</Text>
      <Text>Pending: {isPending ? 'yes' : 'no'}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-use-id-transition/`
- [ ] Uses `useId` and `useTransition` hooks
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
