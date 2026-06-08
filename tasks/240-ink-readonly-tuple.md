# Task 240: `ink-readonly-tuple` Example — Readonly Tuples and Arrays

**Priority:** P1-High
**Phase:** 21 — TypeScript Type Patterns
**Depends on:** 239

## Problem

Readonly tuples and arrays (`readonly [string, number]`, `ReadonlyArray<T>`, `readonly T[]`) prevent mutation at the type level. Task 087 covers `readonly` in objects but not readonly tuples/arrays.

## Ink Example

```tsx
// examples/ink-readonly-tuple/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const pair: readonly [string, number] = ['value', 42];
const list: ReadonlyArray<string> = ['a', 'b', 'c'];

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Pair: {pair[0]} = {pair[1]}</Text>
      <Text>List: {list.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-readonly-tuple/`
- [ ] Uses `readonly [string, number]` tuple syntax
- [ ] Uses `ReadonlyArray<T>` or `readonly T[]`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases readonly modifiers without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
