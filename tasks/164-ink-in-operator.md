# Task 164: `ink-in-operator` Example — `in` Operator (`prop in obj`, `index in arr`)

**Priority:** P1-High
**Phase:** 16 — Operator Completion
**Depends on:** 050

## Problem

The `in` operator (`prop in obj`, `0 in arr`) checks for property existence. It is used in 8+ existing examples but has **no dedicated task** verifying its behavior across all 3 environments.

## Ink Example

```tsx
// examples/ink-in-operator/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const obj = { name: 'App', version: 1 };
const arr = ['a', 'b', 'c'];

const hasName = 'name' in obj;
const hasVersion = 'version' in obj;
const hasMissing = 'missing' in obj;
const hasIndex0 = 0 in arr;
const hasIndex5 = 5 in arr;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Has name: {hasName ? 'yes' : 'no'}</Text>
      <Text>Has version: {hasVersion ? 'yes' : 'no'}</Text>
      <Text>Has missing: {hasMissing ? 'yes' : 'no'}</Text>
      <Text>Index 0: {hasIndex0 ? 'yes' : 'no'}</Text>
      <Text>Index 5: {hasIndex5 ? 'yes' : 'no'}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-in-operator/`
- [ ] Uses `in` operator with object properties
- [ ] Uses `in` operator with array indices
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for `in` operator
- [ ] Parity harness passes with 100% match in all 3 environments
