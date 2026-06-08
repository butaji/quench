# Task 331: `ink-use-callback-deps` Example — `useCallback` with Dependencies

**Priority:** P1-High
**Phase:** 27 — React Hook Patterns
**Depends on:** 330

## Problem

`useCallback(fn, deps)` memoizes a callback and only recreates it when dependencies change. No dedicated Ink example exercises dependency-driven callback memoization.

## Ink Example

```tsx
// examples/ink-use-callback-deps/tui/app.tsx
import React, { useState, useCallback } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [count, setCount] = useState(0);
  const [multiplier, setMultiplier] = useState(1);

  const increment = useCallback(() => {
    setCount(c => c + multiplier);
  }, [multiplier]);

  return (
    <Box flexDirection="column">
      <Text>Count: {count}</Text>
      <Text>Multiplier: {multiplier}</Text>
      <Text>Callback set</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-use-callback-deps/`
- [ ] Uses `useCallback` with explicit dependency array
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
