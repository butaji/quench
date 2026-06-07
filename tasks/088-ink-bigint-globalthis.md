# Task 088: `ink-bigint-globalthis` Example — BigInt, Numeric Separators, `globalThis`

**Priority:** P2-Medium
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

ES2020+ features BigInt (arbitrary precision integers), numeric separators (`1_000_000`), and `globalThis` (universal global object) are not exercised by any existing Ink example.

## Ink Example

```tsx
// examples/ink-bigint-globalthis/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const big = 9007199254740993n;
const formatted = 1_000_000_000;
const isNode = typeof globalThis.process !== 'undefined';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>BigInt: {String(big)}</Text>
      <Text>Formatted: {formatted}</Text>
      <Text>Node env: {isNode ? 'yes' : 'no'}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-bigint-globalthis/`
- [ ] Uses BigInt literal (`123n`)
- [ ] Uses numeric separators (`1_000_000`)
- [ ] Uses `globalThis`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles BigInt, numeric separators, and `globalThis`
- [ ] Parity harness passes with 100% match in all 3 environments