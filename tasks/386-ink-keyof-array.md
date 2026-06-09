# Task 386: `ink-keyof-array` Example — `keyof` on Arrays and Tuples

**Priority:** P2-Medium
**Phase:** 31 — Advanced TS/TSX + React Edge Cases
**Depends on:** 385

## Problem

TypeScript's `keyof` operator works on array and tuple types, producing `"length" | "toString" | ... | number`. No existing Ink example explicitly exercises this edge case.

## HIR Coverage

- `keyof` on array/tuple types is erased during type erasure.
- The parser must handle `keyof` on any type expression without errors.

## Compile-Path Codegen

- No runtime codegen is required.
- `keyof` is erased at parse time.

## Ink Example

```tsx
// examples/ink-keyof-array/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

type ArrKeys = keyof string[];
type TupleKeys = keyof [string, number];

const arr = ['a', 'b', 'c'];

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Length: {arr.length}</Text>
      <Text>Items: {arr.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-keyof-array/`
- [ ] Uses `keyof` on array and tuple types
- [ ] Types are erased without runtime impact
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
