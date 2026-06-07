# Task 104: `ink-array-modern` Example — Modern Array Methods

**Priority:** P1-High
**Phase:** 11 — Runtime API Coverage
**Depends on:** 078

## Problem

Modern array methods (`flat`, `flatMap`, `at`, `toSorted`, `toReversed`, `toSpliced`, `with`, `findLast`, `findLastIndex`, `includes`) are standard ES2022+ features. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-array-modern/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const nested = [[1, 2], [3, 4]];
const flat = nested.flat();

const words = ['hello', 'world'];
const mapped = words.flatMap(w => [w, w.length]);

const nums = [10, 20, 30];
const first = nums.at(0);
const last = nums.at(-1);

const unsorted = [3, 1, 2];
const sorted = unsorted.toSorted();
const reversed = unsorted.toReversed();

const hasTwo = nums.includes(20);
const found = nums.findLast(n => n > 15);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Flat: {flat.join(', ')}</Text>
      <Text>Mapped: {mapped.join(', ')}</Text>
      <Text>First: {first}, Last: {last}</Text>
      <Text>Sorted: {sorted.join(', ')}</Text>
      <Text>Reversed: {reversed.join(', ')}</Text>
      <Text>Has 20: {hasTwo ? 'yes' : 'no'}</Text>
      <Text>FindLast &gt;15: {found}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-array-modern/`
- [ ] Uses `flat`, `flatMap`, `at`, `toSorted`, `toReversed`, `includes`, `findLast`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for all array methods
- [ ] Parity harness passes with 100% match in all 3 environments
