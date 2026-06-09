# Task 408: `ink-array-fundamental` Example — `map`, `filter`, `forEach`, `concat`, `join`, `slice`

**Priority:** P1-High
**Phase:** 33 — HIR & Codegen Edge Cases
**Depends on:** 407

## Problem

Fundamental `Array.prototype` methods (`map`, `filter`, `forEach`, `concat`, `join`, `slice`) are used in virtually every real-world application. While some array methods are covered by Tasks 104, 146, 172, 173, 196, and 361, the most commonly used functional iteration methods are not explicitly exercised by a dedicated Ink example.

## HIR Coverage

- `Expr::Call` with `Expr::Member` callee for prototype method chains
- Arrow function callbacks passed to `map`, `filter`, `forEach`
- Array literal expressions and chained method calls

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for method call expressions
- Runtime API mapping for `Array.prototype` methods in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-array-fundamental/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const nums = [1, 2, 3, 4, 5];
const doubled = nums.map((n) => n * 2);
const evens = nums.filter((n) => n % 2 === 0);
const joined = nums.join('-');
const sliced = nums.slice(1, 4);
const concated = nums.concat([6, 7]);

const results: string[] = [];
nums.forEach((n) => results.push(`item:${n}`));

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Doubled: {doubled.join(', ')}</Text>
      <Text>Evens: {evens.join(', ')}</Text>
      <Text>Joined: {joined}</Text>
      <Text>Sliced: {sliced.join(', ')}</Text>
      <Text>Concated: {concated.join(', ')}</Text>
      <Text>ForEach: {results.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-array-fundamental/`
- [ ] Uses `map`, `filter`, `forEach`, `concat`, `join`, `slice`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
