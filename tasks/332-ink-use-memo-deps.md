# Task 332: `ink-use-memo-deps` Example — `useMemo` with Dependencies

**Priority:** P1-High
**Phase:** 27 — React Hook Patterns
**Depends on:** 331

## Problem

`useMemo(factory, deps)` memoizes a computed value and only recomputes when dependencies change. No dedicated Ink example exercises dependency-driven value memoization.

## Ink Example

```tsx
// examples/ink-use-memo-deps/tui/app.tsx
import React, { useState, useMemo } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [n, setN] = useState(5);

  const factorial = useMemo(() => {
    let result = 1;
    for (let i = 2; i <= n; i++) result *= i;
    return result;
  }, [n]);

  return (
    <Box flexDirection="column">
      <Text>N: {n}</Text>
      <Text>Factorial: {factorial}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-use-memo-deps/`
- [ ] Uses `useMemo` with explicit dependency array
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
