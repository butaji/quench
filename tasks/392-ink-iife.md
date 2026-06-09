# Task 392: `ink-iife` Example — Immediately Invoked Function Expressions

**Priority:** P1-High
**Phase:** 32 — Core Language + React + Runtime Edge Cases
**Depends on:** 391

## Problem

IIFE (`(function() { ... })()` or `(() => { ... })()`) is a common pattern for creating local scope and executing code immediately. No existing Ink example explicitly exercises this pattern.

## HIR Coverage

- `Expr::Call` with `Expr::Function` or `Expr::Arrow` as the callee.
- Anonymous function expressions wrapped in parentheses.

## Compile-Path Codegen

- `quote_codegen_exprs.inc` must emit compilable Rust for function expressions invoked immediately.
- Generated code must preserve the scoping of the IIFE body.

## Ink Example

```tsx
// examples/ink-iife/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const result = (() => {
  const x = 10;
  const y = 20;
  return x + y;
})();

const message = (function(name: string) {
  return `Hello, ${name}`;
})('World');

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Result: {result}</Text>
      <Text>{message}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-iife/`
- [ ] Uses arrow IIFE `(() => { ... })()` and function IIFE `(function() { ... })()`
- [ ] HIR `Expr::Call` with function callee produces compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
