# Task 420: `ink-bigint-proto` Example — `BigInt.prototype.toString`, `BigInt.prototype.valueOf`

**Priority:** P2-Medium
**Phase:** 33 — HIR & Codegen Edge Cases
**Depends on:** 419

## Problem

`BigInt.prototype` methods (`toString`, `valueOf`) are not explicitly exercised. Task 088 covers BigInt literals and Task 226 covers BigInt operations, but the prototype methods are missing.

## HIR Coverage

- `Expr::Call` with `Expr::Member` callee for `bigint.toString()` and `bigint.valueOf()`
- `Expr::BigInt` literal expressions

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-bigint-proto/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const n = 123456789012345678901234567890n;
const str = n.toString();
const hex = n.toString(16);
const val = n.valueOf();
const added = val + 1n;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Decimal: {str}</Text>
      <Text>Hex: {hex}</Text>
      <Text>ValueOf: {String(val)}</Text>
      <Text>Added: {String(added)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-bigint-proto/`
- [ ] Uses `BigInt.prototype.toString` and `BigInt.prototype.valueOf`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
