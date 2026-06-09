# Task 345: `ink-optional-call` Example — Optional Call Expression (`fn?.()`)

**Priority:** P1-High
**Phase:** 27 — TypeScript/JavaScript Syntax
**Depends on:** 344

## Problem

Optional call expressions (`maybeFn?.()`) safely call a function that may be undefined. No existing Ink example exercises this syntax.

## Ink Example

```tsx
// examples/ink-optional-call/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const fn: ((() => string) | undefined) = () => 'called';
  const missing: ((() => string) | undefined) = undefined;

  const a = fn?.();
  const b = missing?.();

  return (
    <Box flexDirection="column">
      <Text>A: {a}</Text>
      <Text>B: {b ?? 'undefined'}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Expr` variants for operators, literals, and call expressions

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation

## Acceptance Criteria

- [ ] Example exists at `examples/ink-optional-call/`
- [ ] Uses optional call expression `fn?.()`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
