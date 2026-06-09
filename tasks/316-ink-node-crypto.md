# Task 316: `ink-node-crypto` Example — Node.js `crypto` Module

**Priority:** P1-High
**Phase:** 26 — Node.js Standard Library
**Depends on:** 315

## Problem

The Node.js `crypto` module provides cryptographic functions (`createHash`, `randomBytes`, `createHmac`, etc.). Task 248 covers Web Crypto `randomUUID`; no example covers Node.js `crypto`.

## Ink Example

```tsx
// examples/ink-node-crypto/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const hash = crypto.createHash('sha256').update('hello').digest('hex');

  return (
    <Box flexDirection="column">
      <Text>Hash: {hash}</Text>
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

- [ ] Example exists at `examples/ink-node-crypto/`
- [ ] References `crypto` module patterns
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for `crypto` module calls
- [ ] Parity harness passes with 100% match in all 3 environments
