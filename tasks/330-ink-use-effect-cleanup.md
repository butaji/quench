# Task 330: `ink-use-effect-cleanup` Example — `useEffect` Cleanup Functions

**Priority:** P1-High
**Phase:** 27 — React Hook Patterns
**Depends on:** 329

## Problem

`useEffect` cleanup functions run before the effect re-runs or on unmount. No dedicated Ink example exercises cleanup behavior and dependency arrays.

## Ink Example

```tsx
// examples/ink-use-effect-cleanup/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [count, setCount] = useState(0);
  const [log, setLog] = useState<string[]>([]);

  useEffect(() => {
    setLog(prev => [...prev, `mount ${count}`]);
    return () => {
      setLog(prev => [...prev, `cleanup ${count}`]);
    };
  }, [count]);

  useEffect(() => {
    const id = setInterval(() => setCount(c => c + 1), 100);
    return () => clearInterval(id);
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Count: {count}</Text>
      <Text>Log: {log.slice(-3).join(', ')}</Text>
    </Box>
  );
}
```


## HIR Coverage

- React hook calls via `Expr::Call`

## Compile-Path Codegen

- `js_bundle/react_shim.rs` for hook definitions

## Acceptance Criteria

- [ ] Example exists at `examples/ink-use-effect-cleanup/`
- [ ] Uses `useEffect` with cleanup function
- [ ] Uses `useEffect` dependency array
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
