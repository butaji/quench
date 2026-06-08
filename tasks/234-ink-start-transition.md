# Task 234: `ink-start-transition` Example — `startTransition`

**Priority:** P2-Medium
**Phase:** 20 — React Patterns
**Depends on:** 233

## Problem

`startTransition` (React 18+) marks state updates as non-urgent, allowing React to prioritize more urgent updates. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-start-transition/tui/app.tsx
import React, { useState, useTransition } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [count, setCount] = useState(0);
  const [isPending, startTransition] = useTransition();

  function handleClick() {
    startTransition(() => {
      setCount(c => c + 1);
    });
  }

  return (
    <Box flexDirection="column">
      <Text>Count: {count}</Text>
      <Text>Pending: {String(isPending)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-start-transition/`
- [ ] Uses `useTransition` hook with `startTransition`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
