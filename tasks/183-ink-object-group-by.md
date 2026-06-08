# Task 183: `ink-object-group-by` Example — `Object.groupBy`, `Map.groupBy`

**Priority:** P1-High
**Phase:** 17 — ES2024 Features
**Depends on:** 182

## Problem

`Object.groupBy` and `Map.groupBy` (ES2024) group array elements by a key function. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-object-group-by/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const items = [
  { type: 'fruit', name: 'apple' },
  { type: 'veg', name: 'carrot' },
  { type: 'fruit', name: 'banana' },
];

const grouped = (Object as any).groupBy
  ? (Object as any).groupBy(items, (i: any) => i.type)
  : {};

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Groups: {Object.keys(grouped).join(', ')}</Text>
      <Text>Fruits: {(grouped.fruit || []).map((i: any) => i.name).join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-object-group-by/`
- [ ] Uses `Object.groupBy` with callback
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
