# Task 413: `ink-symbol-for` Example — `Symbol.for`, `Symbol.keyFor`, `Symbol.isConcatSpreadable`, `Symbol.unscopables`

**Priority:** P2-Medium
**Phase:** 33 — HIR & Codegen Edge Cases
**Depends on:** 412

## Problem

Global Symbol registry and lesser-known well-known symbols (`Symbol.for`, `Symbol.keyFor`, `Symbol.isConcatSpreadable`, `Symbol.unscopables`) are not explicitly exercised. Task 089 covers basic `Symbol`, Task 155 covers `Symbol.iterator`, Task 186 covers `Symbol.asyncIterator`, and Task 369 covers `toStringTag`/`toPrimitive`/`hasInstance`/`species`, but the global registry and array-behavior symbols are missing.

## HIR Coverage

- `Expr::Call` for `Symbol.for()` and `Symbol.keyFor()`
- `Expr::Member` for well-known symbol properties (`Symbol.isConcatSpreadable`, `Symbol.unscopables`)
- `Expr::ComputedMember` for symbol-keyed property access

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-symbol-for/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const sym1 = Symbol.for('app.symbol');
const sym2 = Symbol.for('app.symbol');
const key = Symbol.keyFor(sym1);

const arr1 = [1, 2];
const arr2 = [3, 4];
(arr2 as any)[Symbol.isConcatSpreadable] = false;
const concat = arr1.concat(arr2);

const obj = {
  a: 1,
  b: 2,
  [Symbol.unscopables]: { a: true },
};

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Same: {sym1 === sym2 ? 'yes' : 'no'}</Text>
      <Text>Key: {key}</Text>
      <Text>Concat: {concat.join(', ')}</Text>
      <Text>Unscopables: {String(Object.keys(obj[Symbol.unscopables] as object).join(','))}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-symbol-for/`
- [ ] Uses `Symbol.for`, `Symbol.keyFor`, `Symbol.isConcatSpreadable`, `Symbol.unscopables`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
