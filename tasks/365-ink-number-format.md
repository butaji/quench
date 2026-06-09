# Task 365: `ink-number-format` Example — `Number.prototype.toExponential` / `toPrecision` / `toFixed`

**Priority:** P2-Medium
**Phase:** 29 — Number Methods Completion
**Depends on:** 364

## Problem

`Number.prototype.toExponential`, `toPrecision`, and `toFixed` format numbers as strings. No dedicated Ink example exercises all three formatting methods.

## Ink Example

```tsx
// examples/ink-number-format/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const n = 1234.5678;

  return (
    <Box flexDirection="column">
      <Text>toFixed(2): {n.toFixed(2)}</Text>
      <Text>toExponential(2): {n.toExponential(2)}</Text>
      <Text>toPrecision(4): {n.toPrecision(4)}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Expr` variants for operators, literals, and call expressions

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-number-format/`
- [ ] Uses `toFixed`, `toExponential`, `toPrecision`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
