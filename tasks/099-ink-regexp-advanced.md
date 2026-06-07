# Task 099: `ink-regexp-advanced` Example — RegExp with Flags, `matchAll`

**Priority:** P3-Low
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

Advanced RegExp features including the `d` (indices) flag (ES2022) and `String.prototype.matchAll` (ES2020) are not exercised by any existing Ink example.

## Ink Example

```tsx
// examples/ink-regexp-advanced/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const text = 'Hello world hello';
const matches = [...text.matchAll(/hello/gi)];
const count = matches.length;

const digits = '1,2,3';
const split = digits.split(/,/g);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Matches: {count}</Text>
      <Text>Split: {split.join('|')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-regexp-advanced/`
- [ ] Uses `String.prototype.matchAll` with global RegExp
- [ ] Uses `String.prototype.split` with RegExp
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
