# Task 244: `ink-resolve-json-module` Example — `resolveJsonModule`

**Priority:** P1-High
**Phase:** 21 — TypeScript Configuration
**Depends on:** 243

## Problem

`resolveJsonModule` allows importing `.json` files as typed modules. Task 180 covers import attributes for JSON, but `resolveJsonModule` is the traditional TypeScript option. No dedicated example exists.

## Ink Example

```tsx
// examples/ink-resolve-json-module/config.json
{ "name": "App", "version": "1.0.0" }

// examples/ink-resolve-json-module/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';
import config from '../config.json';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Name: {config.name}</Text>
      <Text>Version: {config.version}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Stmt` variants for control flow and declarations
- `Import`, `Export`, `ExportAll` statement variants
- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- `quote_codegen_stmts.inc` for statement codegen
- `quote_codegen_stmts.inc` + bundler for module resolution
- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-resolve-json-module/`
- [ ] Includes `tsconfig.json` with `resolveJsonModule: true`
- [ ] Imports a `.json` file
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles JSON module imports
- [ ] Parity harness passes with 100% match in all 3 environments
