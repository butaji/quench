# Task 267: `ink-ts-nocheck` Example — `// @ts-nocheck` and `// @ts-check`

**Priority:** P2-Medium
**Phase:** 22 — TypeScript Compiler Directives
**Depends on:** 266

## Problem

`// @ts-nocheck` disables type checking for an entire file, while `// @ts-check` enables it for plain JS files. Task 197 covers `@ts-expect-error` and `@ts-ignore`; no example covers `@ts-nocheck` / `@ts-check`.

## Ink Example

```tsx
// @ts-nocheck

// examples/ink-ts-nocheck/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  // Intentionally bad type that would error without @ts-nocheck
  const value: number = 'not-a-number' as any;

  return (
    <Box flexDirection="column">
      <Text>Value: {String(value)}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Stmt` variants for control flow and declarations

## Compile-Path Codegen

- `quote_codegen_stmts.inc` for statement codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-ts-nocheck/`
- [ ] Uses `// @ts-nocheck` file-level directive
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path strips `@ts-nocheck` without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
