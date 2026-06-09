# Task 360: `ink-inline-import-type` Example — Inline `import('...').Type` Syntax

**Priority:** P1-High
**Phase:** 28 — Module Type Patterns
**Depends on:** 359

## Problem

Inline `import('...').Type` syntax allows referencing types without top-level imports. Task 150 covers `type T = import('./mod').Type`; no example exercises inline usage.

## Ink Example

```tsx
// examples/ink-inline-import-type/lib.ts
export interface Message {
  text: string;
}

// examples/ink-inline-import-type/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const msg: import('./lib').Message = { text: 'inline import type' };

  return (
    <Box flexDirection="column">
      <Text>{msg.text}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Stmt` variants for control flow and declarations
- `Import`, `Export`, `ExportAll` statement variants

## Compile-Path Codegen

- `quote_codegen_stmts.inc` for statement codegen
- `quote_codegen_stmts.inc` + bundler for module resolution

## Acceptance Criteria

- [ ] Example exists at `examples/ink-inline-import-type/`
- [ ] Uses inline `import('./path').Type` syntax
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases inline import types without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
