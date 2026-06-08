# Task 361: `ink-array-flat-flatmap` Example — `Array.prototype.flat` and `flatMap`

**Priority:** P1-High
**Phase:** 29 — Array Methods Completion
**Depends on:** 360

## Problem

`Array.prototype.flat(depth)` and `Array.prototype.flatMap(mapper)` flatten nested arrays and map+flatten in one step. Task 104 covers `flat`/`flatMap` alongside other modern methods; no dedicated example isolates them.

## Ink Example

```tsx
// examples/ink-array-flat-flatmap/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const nested = [1, [2, 3], [[4, 5]]];
  const flat1 = (nested as any).flat ? (nested as any).flat(1) : nested;
  const flat2 = (nested as any).flat ? (nested as any).flat(2) : nested;

  const mapped = [1, 2, 3];
  const flatMapped = (mapped as any).flatMap
    ? (mapped as any).flatMap((x: number) => [x, x * 2])
    : mapped;

  return (
    <Box flexDirection="column">
      <Text>Flat(1): {flat1.join(', ')}</Text>
      <Text>Flat(2): {flat2.join(', ')}</Text>
      <Text>FlatMap: {flatMapped.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-array-flat-flatmap/`
- [ ] Uses `Array.prototype.flat` with depth
- [ ] Uses `Array.prototype.flatMap`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
