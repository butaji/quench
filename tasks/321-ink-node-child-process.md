# Task 321: `ink-node-child-process` Example — Node.js `child_process` Module

**Priority:** P2-Medium
**Phase:** 26 — Node.js Standard Library
**Depends on:** 320

## Problem

The Node.js `child_process` module spawns subprocesses (`spawn`, `exec`, `fork`). No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-node-child-process/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Child process spawned</Text>
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

- [ ] Example exists at `examples/ink-node-child-process/`
- [ ] References `child_process` patterns
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
