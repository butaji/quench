# Task 387: `ink-forof-entries` Example — `for...of` with `.entries()`, `.keys()`, `.values()`

**Priority:** P1-High
**Phase:** 31 — Advanced TS/TSX + React Edge Cases
**Depends on:** 386

## Problem

`for...of` loops can iterate over `.entries()`, `.keys()`, and `.values()` iterators from arrays, maps, sets, and objects. No existing Ink example explicitly exercises all three iterator methods in a single example.

## HIR Coverage

- `Stmt::ForOf` with destructured `[key, value]` patterns.
- `Expr::Call` for `.entries()`, `.keys()`, `.values()` method calls.

## Compile-Path Codegen

- `quote_codegen_stmts.inc` must emit compilable Rust for `for...of` with destructured array patterns.
- `quote_codegen_exprs.inc` must emit compilable Rust for method chain calls.

## Ink Example

```tsx
// examples/ink-forof-entries/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const map = new Map([['a', 1], ['b', 2], ['c', 3]]);
const set = new Set(['x', 'y', 'z']);

export default function App() {
  const entries: string[] = [];
  const keys: string[] = [];
  const values: string[] = [];

  for (const [k, v] of map.entries()) {
    entries.push(`${k}=${v}`);
  }

  for (const k of map.keys()) {
    keys.push(k);
  }

  for (const v of set.values()) {
    values.push(v);
  }

  return (
    <Box flexDirection="column">
      <Text>Entries: {entries.join(', ')}</Text>
      <Text>Keys: {keys.join(', ')}</Text>
      <Text>Values: {values.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-forof-entries/`
- [ ] Uses `for...of` with `.entries()`, `.keys()`, and `.values()`
- [ ] HIR `ForOf` with destructured array pattern produces compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
