# Task 235: `ink-react-19-hooks` Example — React 19 Hooks (`useFormStatus`, `useOptimistic`, `useActionState`, `use`)

**Priority:** P3-Low
**Phase:** 20 — React Patterns
**Depends on:** 234

## Problem

React 19 introduces new hooks: `useFormStatus`, `useOptimistic`, `useActionState`, and the `use` API. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-react-19-hooks/tui/app.tsx
import React, { useOptimistic, useState } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [state, setState] = useState<string[]>(['item1']);
  const [optimisticState, addOptimistic] = useOptimistic(
    state,
    (current, newItem: string) => [...current, newItem]
  );

  function addItem() {
    const newItem = `item${state.length + 1}`;
    addOptimistic(newItem);
    setState(s => [...s, newItem]);
  }

  return (
    <Box flexDirection="column">
      <Text>Items: {optimisticState.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-react-19-hooks/`
- [ ] Uses `useOptimistic` hook
- [ ] Optionally uses `useFormStatus`, `useActionState`, or `use`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
