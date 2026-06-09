# Task 402: `ink-console-table` Example — console.table with Complex Data

**Priority:** P2-Medium
**Phase:** 32 — Core Language + React + Runtime Edge Cases
**Depends on:** 401

## Problem

`console.table` renders tabular data. No existing Ink example explicitly exercises this console method.

## HIR Coverage

- `Expr::Call` for `console.table` with object/array argument.
- Standard member expression for `console.table`.

## Compile-Path Codegen

- `quote_codegen_exprs.inc` must emit compilable Rust for `console.table`.
- May map to a formatted println or no-op depending on compile-path console strategy.

## Ink Example

```tsx
// examples/ink-console-table/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const users = [
  { name: 'Alice', age: 30 },
  { name: 'Bob', age: 25 },
  { name: 'Carol', age: 35 }
];

console.table(users);
console.table(users, ['name']);

export default function App() {
  return (
    <Box>
      <Text>Users: {users.length}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-console-table/`
- [ ] Uses `console.table` with array of objects
- [ ] HIR `Expr::Call` for `console.table` produces compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust or no-op
- [ ] Parity harness passes with 100% match in all 3 environments
