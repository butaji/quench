# Task 162: `ink-key-prop` Example — `key` Prop in Lists and Fragments

**Priority:** P1-High
**Phase:** 14 — React Pattern Coverage
**Depends on:** 137

## Problem

The `key` prop is used in 40+ existing examples for list rendering and fragment identification, but there is **no dedicated task** verifying its behavior across all 3 environments. The `key` prop is essential for React's reconciliation algorithm.

## Ink Example

```tsx
// examples/ink-key-prop/tui/app.tsx
import React, { useState } from 'react';
import { Box, Text } from 'ink';

interface Item {
  id: number;
  label: string;
}

export default function App() {
  const [items, setItems] = useState<Item[]>([
    { id: 1, label: 'First' },
    { id: 2, label: 'Second' },
    { id: 3, label: 'Third' },
  ]);

  return (
    <Box flexDirection="column">
      {items.map(item => (
        <Text key={item.id}>{item.label}</Text>
      ))}
      <Box key="separator" height={1} />
      {items.map((item, index) => (
        <Text key={`alt-${item.id}`}>{index}: {item.label}</Text>
      ))}
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-key-prop/`
- [ ] Uses `key` prop with stable ids in `.map()`
- [ ] Uses `key` prop with string template expressions
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] `key` prop is stripped or handled correctly in compile path
- [ ] Parity harness passes with 100% match in all 3 environments
