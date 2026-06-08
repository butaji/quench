# Task 192: `ink-function-expression` Example — Anonymous Function Expressions

**Priority:** P1-High
**Phase:** 17 — Expression-Level TypeScript Features
**Depends on:** 191

## Problem

Anonymous function expressions (`const fn = function(x) { ... }`) and named function expressions (`const fn = function name(x) { ... }`) exercise HIR function handling in expression position. No existing Ink example explicitly targets function expressions.

## Ink Example

```tsx
// examples/ink-function-expression/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const add = function (a: number, b: number): number {
  return a + b;
};

const multiply = function mul(a: number, b: number): number {
  return a * b;
};

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Add: {add(2, 3)}</Text>
      <Text>Multiply: {multiply(4, 5)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-function-expression/`
- [ ] Uses anonymous function expression
- [ ] Optionally uses named function expression
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
