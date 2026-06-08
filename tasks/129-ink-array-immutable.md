# Task 129: `ink-array-immutable` Example — `toSpliced`, `with` (ES2023)

**Priority:** P1-High
**Phase:** 12 — ES2023 Language Features
**Depends on:** 128

## Problem

Immutable array methods `toSpliced()` and `with()` (ES2023) allow non-mutating versions of `splice` and index assignment. Task 104 covers `toSorted`/`toReversed` but not these. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-array-immutable/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const original = [1, 2, 3, 4, 5];
const spliced = original.toSpliced(1, 2, 'a', 'b');
const replaced = original.with(2, 'X');

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Original: {original.join(', ')}</Text>
      <Text>Spliced: {spliced.join(', ')}</Text>
      <Text>Replaced: {replaced.join(', ')}</Text>
      <Text>Unchanged: {original.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-array-immutable/`
- [ ] Uses `Array.prototype.toSpliced`
- [ ] Uses `Array.prototype.with`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
