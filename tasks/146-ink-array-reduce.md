# Task 146: `ink-array-reduce` Example — `reduce`, `reduceRight`

**Priority:** P1-High
**Phase:** 12 — Runtime API Completion
**Depends on:** 145

## Problem

`Array.prototype.reduce` and `reduceRight` are fundamental functional programming methods. No existing Ink example explicitly exercises them.

## Ink Example

```tsx
// examples/ink-array-reduce/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const nums = [1, 2, 3, 4, 5];
const sum = nums.reduce((acc, n) => acc + n, 0);
const product = nums.reduce((acc, n) => acc * n, 1);
const max = nums.reduce((acc, n) => (n > acc ? n : acc), nums[0]);
const reversed = nums.reduceRight((acc, n) => [...acc, n], [] as number[]);

const entries = [['a', 1], ['b', 2], ['c', 3]] as [string, number][];
const obj = entries.reduce((acc, [k, v]) => ({ ...acc, [k]: v }), {} as Record<string, number>);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Sum: {sum}</Text>
      <Text>Product: {product}</Text>
      <Text>Max: {max}</Text>
      <Text>Reversed: {reversed.join(', ')}</Text>
      <Text>Keys: {Object.keys(obj).join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-array-reduce/`
- [ ] Uses `Array.prototype.reduce` with different operations
- [ ] Uses `Array.prototype.reduceRight`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
