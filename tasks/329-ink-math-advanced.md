# Task 329: `ink-math-advanced` Example — Advanced `Math` Methods and Constants

**Priority:** P2-Medium
**Phase:** 26 — Runtime API Completion
**Depends on:** 328

## Problem

Advanced `Math` methods (`sign`, `trunc`, `cbrt`, `hypot`, `log10`, `log2`, `clz32`, `imul`, `fround`) and constants (`E`, `LN10`, `LN2`, `LOG10E`, `LOG2E`, `SQRT1_2`, `SQRT2`) are not covered by any existing task.

## Ink Example

```tsx
// examples/ink-math-advanced/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>sign(-5): {Math.sign(-5)}</Text>
      <Text>trunc(3.9): {Math.trunc(3.9)}</Text>
      <Text>cbrt(27): {Math.cbrt(27)}</Text>
      <Text>hypot(3,4): {Math.hypot(3, 4)}</Text>
      <Text>log10(100): {Math.log10(100)}</Text>
      <Text>log2(8): {Math.log2(8)}</Text>
      <Text>E: {Math.E.toFixed(2)}</Text>
      <Text>LN2: {Math.LN2.toFixed(2)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-math-advanced/`
- [ ] Uses advanced `Math` methods and constants
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
