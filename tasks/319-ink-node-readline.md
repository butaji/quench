# Task 319: `ink-node-readline` Example — Node.js `readline` Module

**Priority:** P1-High
**Phase:** 26 — Node.js Standard Library
**Depends on:** 318

## Problem

The Node.js `readline` module provides line-by-line reading from streams. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-node-readline/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const lines = ['line1', 'line2', 'line3'];

  return (
    <Box flexDirection="column">
      <Text>Lines: {lines.join(', ')}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Stmt` variants for control flow and declarations
- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- `quote_codegen_stmts.inc` for statement codegen
- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-node-readline/`
- [ ] References `readline` module patterns
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
