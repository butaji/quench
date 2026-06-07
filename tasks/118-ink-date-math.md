# Task 118: `ink-date-math` Example — `Date`, `Math`, `Intl`

**Priority:** P2-Medium
**Phase:** 11 — Runtime API Coverage
**Depends on:** 078

## Problem

`Date`, `Math`, and `Intl` are fundamental JavaScript global objects. No existing Ink example exercises them in a TUI context.

## Ink Example

```tsx
// examples/ink-date-math/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const now = new Date();
const iso = now.toISOString().slice(0, 10);
const time = now.toLocaleTimeString();

const sqrt = Math.sqrt(16);
const pow = Math.pow(2, 10);
const floor = Math.floor(3.7);
const random = Math.random();
const max = Math.max(1, 5, 3, 9);

// Intl.DateTimeFormat if available
const formatted = Intl.DateTimeFormat
  ? new Intl.DateTimeFormat('en-US', { dateStyle: 'short' }).format(now)
  : iso;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Date: {iso}</Text>
      <Text>Time: {time}</Text>
      <Text>Sqrt: {sqrt}</Text>
      <Text>Pow: {pow}</Text>
      <Text>Floor: {floor}</Text>
      <Text>Max: {max}</Text>
      <Text>Formatted: {formatted}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-date-math/`
- [ ] Uses `Date` object methods
- [ ] Uses `Math` object methods
- [ ] Optionally uses `Intl.DateTimeFormat`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
