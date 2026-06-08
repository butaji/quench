# Task 167: `ink-array-static` Example — `Array.from`, `Array.of`, `Array.isArray`

**Priority:** P1-High
**Phase:** 16 — Runtime API Completion
**Depends on:** 166

## Problem

`Array.from`, `Array.of`, and `Array.isArray` are essential Array static methods. No existing Ink example explicitly exercises all three.

## Ink Example

```tsx
// examples/ink-array-static/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const fromIterable = Array.from('hello');
const fromMap = Array.from([1, 2, 3], x => x * 2);
const ofValues = Array.of(1, 2, 3);
const isArr = Array.isArray([1, 2, 3]);
const isNotArr = Array.isArray('hello');

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>From iterable: {fromIterable.join(', ')}</Text>
      <Text>From map: {fromMap.join(', ')}</Text>
      <Text>Of: {ofValues.join(', ')}</Text>
      <Text>Is array: {isArr ? 'yes' : 'no'}</Text>
      <Text>Is not array: {isNotArr ? 'yes' : 'no'}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-array-static/`
- [ ] Uses `Array.from`, `Array.of`, `Array.isArray`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
