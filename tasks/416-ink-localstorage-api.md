# Task 416: `ink-localstorage-api` Example — `getItem`, `setItem`, `removeItem`, `clear`, `length`, `key`

**Priority:** P2-Medium
**Phase:** 33 — HIR & Codegen Edge Cases
**Depends on:** 415

## Problem

The full `localStorage` API (`getItem`, `setItem`, `removeItem`, `clear`, `length`, `key`) is not comprehensively exercised. Task 325 covers basic `getItem`/`setItem`, but `removeItem`, `clear`, `length`, and `key` are missing.

## HIR Coverage

- `Expr::Call` with `Expr::Member` callee for `localStorage.*` methods
- `Expr::Member` for `localStorage.length`
- `Expr::Call` for `localStorage.key(index)`

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-localstorage-api/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

localStorage.setItem('a', '1');
localStorage.setItem('b', '2');
localStorage.setItem('c', '3');
localStorage.removeItem('b');

const len = localStorage.length;
const firstKey = localStorage.key(0);
const a = localStorage.getItem('a');
const b = localStorage.getItem('b');

localStorage.clear();
const afterClear = localStorage.length;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Length: {len}</Text>
      <Text>FirstKey: {firstKey}</Text>
      <Text>A: {a}</Text>
      <Text>B: {b}</Text>
      <Text>AfterClear: {afterClear}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-localstorage-api/`
- [ ] Uses `getItem`, `setItem`, `removeItem`, `clear`, `length`, `key`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
