# Task 368: `ink-error-stack` Example — `Error.captureStackTrace` and `Error.stackTraceLimit`

**Priority:** P2-Medium
**Phase:** 29 — Error API Completion
**Depends on:** 367

## Problem

`Error.captureStackTrace` and `Error.stackTraceLimit` control stack trace generation in V8. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-error-stack/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const err = new Error('test');
  const hasStack = !!err.stack;

  return (
    <Box flexDirection="column">
      <Text>Has stack: {String(hasStack)}</Text>
      <Text>Message: {err.message}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Expr` variants for operators, literals, and call expressions

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation

## Acceptance Criteria

- [ ] Example exists at `examples/ink-error-stack/`
- [ ] References `Error.captureStackTrace` / `stackTraceLimit`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
