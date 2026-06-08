# Task 173: `ink-array-searchers` Example — `indexOf`, `lastIndexOf`, `every`, `some`, `filter`, `find`, `findIndex`

**Priority:** P1-High
**Phase:** 16 — Runtime API Completion
**Depends on:** 172

## Problem

Array search/query methods (`indexOf`, `lastIndexOf`, `every`, `some`, `filter`, `find`, `findIndex`) are commonly used. No existing Ink example explicitly exercises all of them.

## Ink Example

```tsx
// examples/ink-array-searchers/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const nums = [1, 2, 3, 2, 4];

const idx = nums.indexOf(2);
const lastIdx = nums.lastIndexOf(2);
const allPositive = nums.every(n => n > 0);
const hasEven = nums.some(n => n % 2 === 0);
const evens = nums.filter(n => n % 2 === 0);
const firstEven = nums.find(n => n % 2 === 0);
const firstEvenIdx = nums.findIndex(n => n % 2 === 0);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Index: {idx}, Last: {lastIdx}</Text>
      <Text>All positive: {allPositive ? 'yes' : 'no'}</Text>
      <Text>Has even: {hasEven ? 'yes' : 'no'}</Text>
      <Text>Evens: {evens.join(', ')}</Text>
      <Text>First even: {firstEven}</Text>
      <Text>First even idx: {firstEvenIdx}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-array-searchers/`
- [ ] Uses `indexOf`, `lastIndexOf`, `every`, `some`, `filter`, `find`, `findIndex`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
