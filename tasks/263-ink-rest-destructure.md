# Task 263: `ink-rest-destructure` Example — Rest Elements in Object/Array Destructuring

**Priority:** P1-High
**Phase:** 22 — Advanced Language Features
**Depends on:** 262

## Problem

Rest elements in destructuring (`const { a, ...rest } = obj`, `const [first, ...rest] = arr`) collect remaining properties/elements. No dedicated example exercises this pattern.

## Ink Example

```tsx
// examples/ink-rest-destructure/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const obj = { a: 1, b: 2, c: 3 };
  const arr = [10, 20, 30, 40];

  const { a, ...objRest } = obj;
  const [first, ...arrRest] = arr;

  return (
    <Box flexDirection="column">
      <Text>A: {a}</Text>
      <Text>Obj rest: {Object.keys(objRest).join(', ')}</Text>
      <Text>First: {first}</Text>
      <Text>Arr rest: {arrRest.join(', ')}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Stmt` variants for control flow and declarations

## Compile-Path Codegen

- `quote_codegen_stmts.inc` for statement codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-rest-destructure/`
- [ ] Uses rest in object destructuring
- [ ] Uses rest in array destructuring
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for rest destructuring
- [ ] Parity harness passes with 100% match in all 3 environments
