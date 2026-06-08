# Task 157: `ink-performance` Example — `performance.now()`, `performance.mark`

**Priority:** P2-Medium
**Phase:** 14 — Runtime API Completion
**Depends on:** 156

## Problem

`performance.now()` and `performance.mark()` are standard timing APIs for high-resolution measurements. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-performance/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const start = performance.now();
let sum = 0;
for (let i = 0; i < 1000000; i++) {
  sum += i;
}
const end = performance.now();
const elapsed = (end - start).toFixed(2);

performance.mark('start');
performance.mark('end');
const measure = performance.measure('loop', 'start', 'end');

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Sum: {sum}</Text>
      <Text>Elapsed: {elapsed}ms</Text>
      <Text>Measure: {measure.duration.toFixed(2)}ms</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-performance/`
- [ ] Uses `performance.now()` for timing
- [ ] Uses `performance.mark()` and `performance.measure()`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
