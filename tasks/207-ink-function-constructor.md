# Task 207: `ink-function-constructor` Example — `Function` Constructor

**Priority:** P2-Medium
**Phase:** 19 — Advanced Expression Patterns
**Depends on:** 206

## Problem

The `Function` constructor (`new Function('a', 'b', 'return a + b')`) dynamically creates functions from strings. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-function-constructor/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const add = new Function('a', 'b', 'return a + b') as (a: number, b: number) => number;
  const result = add(2, 3);

  return (
    <Box flexDirection="column">
      <Text>Result: {result}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Expr` variants for operators, literals, and call expressions
- `ClassMember` and `Class` variants

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- `quote_codegen.rs` for class declaration codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-function-constructor/`
- [ ] Uses `new Function(...)` constructor
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for Function constructor
- [ ] Parity harness passes with 100% match in all 3 environments
