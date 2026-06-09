# Task 344: `ink-custom-hook-composition` Example — Composing Multiple Custom Hooks

**Priority:** P1-High
**Phase:** 27 — React Hook Patterns
**Depends on:** 343

## Problem

Custom hook composition (`useCounter` + `useLogger`) is a common pattern for building reusable behavior. No dedicated Ink example exercises composing multiple custom hooks.

## Ink Example

```tsx
// examples/ink-custom-hook-composition/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

function useCounter(initial: number) {
  const [count, setCount] = useState(initial);
  useEffect(() => {
    const id = setInterval(() => setCount(c => c + 1), 200);
    return () => clearInterval(id);
  }, []);
  return count;
}

function useLogger(value: number) {
  useEffect(() => {}, [value]);
}

export default function App() {
  const count = useCounter(0);
  useLogger(count);

  return (
    <Box flexDirection="column">
      <Text>Count: {count}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr`/`Stmt` variants

## Compile-Path Codegen

- Standard `quote_codegen` expression + statement codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-custom-hook-composition/`
- [ ] Composes at least two custom hooks
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
