# Task 243: `ink-isolated-modules` Example — `isolatedModules` Compiler Option

**Priority:** P1-High
**Phase:** 21 — TypeScript Configuration
**Depends on:** 242

## Problem

`isolatedModules` enforces that each file can be transpiled independently, which is required by Babel/swc/oxc. No existing Ink example exercises this compiler option.

## Ink Example

```tsx
// tsconfig.json with isolatedModules: true
// examples/ink-isolated-modules/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export const greeting = 'Hello';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>{greeting}</Text>
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

- [ ] Example exists at `examples/ink-isolated-modules/`
- [ ] Includes `tsconfig.json` with `isolatedModules: true`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path respects `isolatedModules` constraints
- [ ] Parity harness passes with 100% match in all 3 environments
