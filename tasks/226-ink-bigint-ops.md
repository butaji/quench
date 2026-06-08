# Task 226: `ink-bigint-ops` Example — BigInt Literals and Operations

**Priority:** P1-High
**Phase:** 20 — Advanced Language Features
**Depends on:** 225

## Problem

BigInt literals (`1n`, `0xFFn`) and operations (`+`, `-`, `*`, `/`, `%`, `**`, comparisons) are not covered by any dedicated example. Task 088 covers BigInt but only as a secondary feature.

## Ink Example

```tsx
// examples/ink-bigint-ops/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const a = 9007199254740991n;
  const b = 1n;
  const sum = a + b;
  const diff = a - b;
  const prod = a * 2n;
  const div = a / 3n;
  const rem = a % 100n;
  const pow = 2n ** 64n;
  const eq = a === a;
  const gt = a > b;

  return (
    <Box flexDirection="column">
      <Text>Sum: {String(sum)}</Text>
      <Text>Diff: {String(diff)}</Text>
      <Text>Product: {String(prod)}</Text>
      <Text>Div: {String(div)}</Text>
      <Text>Rem: {String(rem)}</Text>
      <Text>Pow: {String(pow)}</Text>
      <Text>Eq: {String(eq)}</Text>
      <Text>Gt: {String(gt)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-bigint-ops/`
- [ ] Uses BigInt literals and arithmetic operations
- [ ] Uses BigInt comparisons
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for BigInt operations
- [ ] Parity harness passes with 100% match in all 3 environments
