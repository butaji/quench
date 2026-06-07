# Task 106: `ink-use-imperative-handle` Example — `useImperativeHandle`, `forwardRef`

**Priority:** P1-High
**Phase:** 11 — React Hook Coverage
**Depends on:** 078

## Problem

`useImperativeHandle` is a React hook for exposing imperative methods from a child component to its parent via a ref. Combined with `forwardRef`, it's essential for component library design. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-use-imperative-handle/tui/app.tsx
import React, { useRef, useImperativeHandle, forwardRef } from 'react';
import { Box, Text } from 'ink';

interface CounterHandle {
  increment: () => void;
  getValue: () => number;
}

const Counter = forwardRef<CounterHandle>((_props, ref) => {
  const [count, setCount] = React.useState(0);

  useImperativeHandle(ref, () => ({
    increment: () => setCount(c => c + 1),
    getValue: () => count,
  }), [count]);

  return <Text>Count: {count}</Text>;
});

export default function App() {
  const counterRef = useRef<CounterHandle>(null);

  return (
    <Box flexDirection="column">
      <Counter ref={counterRef} />
      <Text>Ref value: {counterRef.current?.getValue() ?? 0}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-use-imperative-handle/`
- [ ] Uses `useImperativeHandle` with `forwardRef`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
