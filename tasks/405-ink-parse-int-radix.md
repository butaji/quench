# Task 405: `ink-parse-int-radix` Example — parseInt with Explicit Radix

**Priority:** P2-Medium
**Phase:** 32 — Core Language + React + Runtime Edge Cases
**Depends on:** 404

## Problem

`parseInt(string, radix)` with explicit radix is a common pattern. No existing Ink example explicitly exercises this with different radix values.

## HIR Coverage

- `Expr::Call` for `parseInt` with two arguments.
- Number literal arguments for radix.

## Compile-Path Codegen

- `quote_codegen_exprs.inc` must emit compilable Rust for `parseInt` with radix.
- May map to Rust string parsing with base specification.

## Ink Example

```tsx
// examples/ink-parse-int-radix/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const bin = parseInt('1010', 2);
const oct = parseInt('77', 8);
const hex = parseInt('FF', 16);
const dec = parseInt('42', 10);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Binary 1010: {bin}</Text>
      <Text>Octal 77: {oct}</Text>
      <Text>Hex FF: {hex}</Text>
      <Text>Decimal 42: {dec}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-parse-int-radix/`
- [ ] Uses `parseInt` with explicit radix (2, 8, 10, 16)
- [ ] HIR `Expr::Call` with two arguments produces compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
