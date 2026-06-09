# Task 238: `ink-nested-destructure` Example — Nested Object/Array Destructuring

**Priority:** P1-High
**Phase:** 21 — Niche Language Features
**Depends on:** 237

## Problem

Nested destructuring patterns (`const { a: { b } } = obj`, `const [[x], [y]] = arr`) extract values from deeply nested structures. Task 045 covers basic destructuring but not nested patterns.

## Ink Example

```tsx
// examples/ink-nested-destructure/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const user = {
    profile: {
      name: 'Alice',
      address: { city: 'NYC' },
    },
    scores: [[10, 20], [30, 40]],
  };

  const {
    profile: {
      name,
      address: { city },
    },
    scores: [[a], [b]],
  } = user;

  return (
    <Box flexDirection="column">
      <Text>Name: {name}</Text>
      <Text>City: {city}</Text>
      <Text>First scores: {a}, {b}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `Stmt` variants for control flow and declarations

## Compile-Path Codegen

- `quote_codegen_stmts.inc` for statement codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-nested-destructure/`
- [ ] Uses nested object destructuring
- [ ] Uses nested array destructuring
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for nested destructuring
- [ ] Parity harness passes with 100% match in all 3 environments
