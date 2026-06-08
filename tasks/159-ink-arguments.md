# Task 159: `ink-arguments` Example — `arguments` Object, Rest vs Arguments

**Priority:** P2-Medium
**Phase:** 14 — Runtime API Completion
**Depends on:** 158

## Problem

The `arguments` object is a legacy JavaScript feature for accessing function parameters. While rest parameters (`...args`) are preferred, `arguments` is still common in older code. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-arguments/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function sumAll(): number {
  let sum = 0;
  for (let i = 0; i < arguments.length; i++) {
    sum += arguments[i];
  }
  return sum;
}

function logArgs() {
  return Array.from(arguments).join(', ');
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Sum(1,2,3): {sumAll(1, 2, 3)}</Text>
      <Text>Sum(10,20): {sumAll(10, 20)}</Text>
      <Text>Args: {logArgs('a', 'b', 'c')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-arguments/`
- [ ] Uses `arguments` object in non-arrow function
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
