# Task 300: `ink-mixed-destructure` Example — Mixed Array/Object Destructuring

**Priority:** P1-High
**Phase:** 25 — Advanced Destructuring Patterns
**Depends on:** 299

## Problem

Mixed destructuring (`const [a, { b }] = [1, { b: 2 }]`) combines array and object destructuring patterns. No existing Ink example exercises this pattern.

## Ink Example

```tsx
// examples/ink-mixed-destructure/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const data = ['first', { value: 42, label: 'answer' }, [10, 20]];
  const [name, { value, label }, [x, y]] = data;

  return (
    <Box flexDirection="column">
      <Text>Name: {name}</Text>
      <Text>Value: {value}</Text>
      <Text>Label: {label}</Text>
      <Text>Coords: {x}, {y}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Stmt` variants for control flow and declarations

## Compile-Path Codegen

- `quote_codegen_stmts.inc` for statement codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-mixed-destructure/`
- [ ] Uses mixed array and object destructuring in one pattern
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for mixed destructuring
- [ ] Parity harness passes with 100% match in all 3 environments
