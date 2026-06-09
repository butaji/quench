# Task 242: `ink-reference-path` Example — `/// <reference path="..." />`

**Priority:** P2-Medium
**Phase:** 21 — TypeScript Configuration
**Depends on:** 241

## Problem

Triple-slash reference path directives (`/// <reference path="./types.d.ts" />`) load ambient type declarations. Task 151 covers `/// <reference types="..." />` but not `reference path`.

## Ink Example

```tsx
/// <reference path="./types.d.ts" />

// examples/ink-reference-path/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

declare const APP_VERSION: string;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Version: {APP_VERSION}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Stmt` variants for control flow and declarations

## Compile-Path Codegen

- `quote_codegen_stmts.inc` for statement codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-reference-path/`
- [ ] Uses `/// <reference path="..." />` directive
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path strips or resolves reference path directives
- [ ] Parity harness passes with 100% match in all 3 environments
