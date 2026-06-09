# Task 282: `ink-package-exports` Example — `package.json` `exports` and `imports`

**Priority:** P2-Medium
**Phase:** 23 — Module Resolution
**Depends on:** 281

## Problem

`package.json` `exports` and `imports` fields control modern module resolution. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-package-exports/package.json
{
  "name": "ink-package-exports-example",
  "exports": {
    ".": "./tui/app.tsx",
    "./config": "./config.json"
  },
  "imports": {
    "#utils/*": "./utils/*"
  }
}

// examples/ink-package-exports/utils/greet.ts
export function greet(name: string): string {
  return `Hello, ${name}`;
}

// examples/ink-package-exports/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';
import { greet } from '#utils/greet';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>{greet('World')}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Stmt` variants for control flow and declarations

## Compile-Path Codegen

- `quote_codegen_stmts.inc` for statement codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-package-exports/`
- [ ] Uses `package.json` `exports` and `imports` fields
- [ ] Imports via package subpath import (`#utils/greet`)
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path resolves `package.json` exports/imports
- [ ] Parity harness passes with 100% match in all 3 environments
