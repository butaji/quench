# Task 196: `ink-array-advanced` Example — `Array` Methods (findLast, findLastIndex, fill, copyWithin, at)

**Priority:** P2-Medium
**Phase:** 17 — Runtime API Deep Coverage
**Depends on:** 195

## Problem

Advanced `Array` prototype methods (`findLast`, `findLastIndex`, `fill`, `copyWithin`, `.at()`) are not covered by any existing task. Task 104 covers modern array methods but not these specific ones.

## Ink Example

```tsx
// examples/ink-array-advanced/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const numbers = [1, 2, 3, 4, 5];

export default function App() {
  const lastEven = numbers.findLast((n) => n % 2 === 0);
  const lastEvenIndex = numbers.findLastIndex((n) => n % 2 === 0);
  const filled = [...numbers].fill(0, 2, 4);
  const copied = [...numbers].copyWithin(0, 3);
  const atIndex = numbers.at(-1);

  return (
    <Box flexDirection="column">
      <Text>LastEven: {lastEven}</Text>
      <Text>LastEvenIndex: {lastEvenIndex}</Text>
      <Text>Filled: {filled.join(', ')}</Text>
      <Text>Copied: {copied.join(', ')}</Text>
      <Text>At(-1): {atIndex}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-array-advanced/`
- [ ] Uses `findLast`, `findLastIndex`, `fill`, `copyWithin`, `at`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
