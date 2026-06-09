# Task 290: `ink-generator-return-throw` Example — `generator.return()` and `generator.throw()`

**Priority:** P2-Medium
**Phase:** 24 — Advanced Language Features
**Depends on:** 289

## Problem

Generator objects expose `return()` and `throw()` methods for early termination and error injection. Task 053 covers generators but not these methods.

## Ink Example

```tsx
// examples/ink-generator-return-throw/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function* range(start: number, end: number) {
  for (let i = start; i <= end; i++) {
    yield i;
  }
}

export default function App() {
  const gen1 = range(1, 10);
  const first = gen1.next().value;
  const returned = gen1.return?.(99);

  const gen2 = range(1, 10);
  let thrown = null;
  try {
    gen2.throw?.(new Error('abort'));
  } catch (e: any) {
    thrown = e.message;
  }

  return (
    <Box flexDirection="column">
      <Text>First: {first}</Text>
      <Text>Return value: {returned?.value}</Text>
      <Text>Thrown: {thrown}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Stmt` variants for control flow and declarations

## Compile-Path Codegen

- `quote_codegen_stmts.inc` for statement codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-generator-return-throw/`
- [ ] Uses `generator.return()` and/or `generator.throw()`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
