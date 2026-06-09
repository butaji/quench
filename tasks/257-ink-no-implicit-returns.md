# Task 257: `ink-no-implicit-returns` Example — `noImplicitReturns` Compiler Option

**Priority:** P2-Medium
**Phase:** 22 — TypeScript Configuration Edge Cases
**Depends on:** 256

## Problem

`noImplicitReturns` requires all code paths in a function to return a value. No existing Ink example exercises this compiler option.

## Ink Example

```tsx
// tsconfig.json with noImplicitReturns: true
// examples/ink-no-implicit-returns/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function getLabel(active: boolean): string {
  if (active) {
    return 'ACTIVE';
  }
  return 'INACTIVE';
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>{getLabel(true)}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Stmt` variants for control flow and declarations
- Parser directives (no runtime HIR impact)

## Compile-Path Codegen

- `quote_codegen_stmts.inc` for statement codegen
- Parser/bundler configuration (no runtime codegen)

## Acceptance Criteria

- [ ] Example exists at `examples/ink-no-implicit-returns/`
- [ ] Includes `tsconfig.json` with `noImplicitReturns: true`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path respects `noImplicitReturns`
- [ ] Parity harness passes with 100% match in all 3 environments
