# Task 424: `ink-array-tolocalestring` Example — `Array.prototype.toLocaleString`, `Number.prototype.toLocaleString`

**Priority:** P2-Medium
**Phase:** 33 — HIR & Codegen Edge Cases
**Depends on:** 423

## Problem

`Array.prototype.toLocaleString` and `Number.prototype.toLocaleString` with locale options are not explicitly exercised. These methods are important for internationalized formatting of collections and numbers.

## HIR Coverage

- `Expr::Call` with `Expr::Member` callee for `toLocaleString`
- `Expr::Object` for locale options (`style`, `currency`, etc.)

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-array-tolocalestring/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const numbers = [1234.5, 6789.0];
const arrStr = numbers.toLocaleString('de-DE');

const num = 1234567.89;
const numStr = num.toLocaleString('en-US', { style: 'currency', currency: 'USD' });

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Array DE: {arrStr}</Text>
      <Text>Number US: {numStr}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-array-tolocalestring/`
- [ ] Uses `Array.prototype.toLocaleString` and `Number.prototype.toLocaleString`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
