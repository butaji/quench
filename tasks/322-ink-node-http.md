# Task 322: `ink-node-http` Example — Node.js `http` / `https` Module

**Priority:** P2-Medium
**Phase:** 26 — Node.js Standard Library
**Depends on:** 321

## Problem

The Node.js `http` and `https` modules create HTTP servers and clients. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-node-http/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>HTTP server created</Text>
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

- [ ] Example exists at `examples/ink-node-http/`
- [ ] References `http` / `https` patterns
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
