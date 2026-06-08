# Task 236: `ink-sequence-expression` Example — Comma Operator Sequences

**Priority:** P2-Medium
**Phase:** 21 — Niche Language Features
**Depends on:** 235

## Problem

The comma operator (`a, b, c`) evaluates each operand left-to-right and returns the last value. Task 177 covers comma operator alongside `void` and `++`/`--`, but no dedicated example exercises sequence expressions explicitly.

## Ink Example

```tsx
// examples/ink-sequence-expression/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  let x = 1;
  let y = 2;
  const result = (x++, y++, x + y);

  return (
    <Box flexDirection="column">
      <Text>X: {x}</Text>
      <Text>Y: {y}</Text>
      <Text>Result: {result}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-sequence-expression/`
- [ ] Uses explicit comma operator sequence `(a, b, c)`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for sequence expressions
- [ ] Parity harness passes with 100% match in all 3 environments
