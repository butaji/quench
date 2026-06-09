# Task 429: `ink-array-sort-compare` Example — `Array.prototype.sort` with Comparator Function

**Priority:** P1-High
**Phase:** 34 — HIR & Codegen Edge Cases
**Depends on:** 428

## Problem

`Array.prototype.sort` with a comparator function (`arr.sort((a, b) => a - b)`) is a fundamental pattern. Task 172 covers `sort` and `reverse` as mutators but does not explicitly exercise the comparator callback. The comparator callback exercises HIR closure-as-argument patterns in method calls.

## HIR Coverage

- `Expr::Call` with `Expr::Member` callee for `arr.sort()`
- `Expr::Arrow` as comparator callback argument
- Comparison expressions inside the comparator

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for method call expressions
- Runtime API mapping for `Array.prototype.sort` with comparator

## Ink Example

```tsx
// examples/ink-array-sort-compare/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const nums = [3, 1, 4, 1, 5, 9, 2, 6];
const asc = [...nums].sort((a, b) => a - b);
const desc = [...nums].sort((a, b) => b - a);

const words = ['banana', 'apple', 'cherry'];
const alpha = [...words].sort((a, b) => a.localeCompare(b));

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Asc: {asc.join(', ')}</Text>
      <Text>Desc: {desc.join(', ')}</Text>
      <Text>Alpha: {alpha.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-array-sort-compare/`
- [ ] Uses `sort` with numeric and string comparators
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
