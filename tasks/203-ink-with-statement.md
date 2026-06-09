# Task 203: `ink-with-statement` Example — `with` Statement (Legacy)

**Priority:** P3-Low
**Phase:** 17 — Legacy JavaScript Features
**Depends on:** 202

## Problem

The `with` statement is a legacy JavaScript feature that extends the scope chain. It is deprecated in strict mode but still appears in some codebases. Testing it exercises HIR scope-chain handling.

## Ink Example

```tsx
// examples/ink-with-statement/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const obj = { a: 1, b: 2 };

  // @ts-ignore
  with (obj) {
    var result = a + b;
  }

  return (
    <Box flexDirection="column">
      <Text>Result: {result}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Stmt` variants for control flow and declarations

## Compile-Path Codegen

- `quote_codegen_stmts.inc` for statement codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-with-statement/`
- [ ] Uses `with` statement
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles `with` or produces clear error
- [ ] Parity harness passes with 100% match in all 3 environments
