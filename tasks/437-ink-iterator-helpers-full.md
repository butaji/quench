# Task 437: `ink-iterator-helpers-full` Example — Full Iterator Helpers API

**Priority:** P1-High
**Phase:** 34 — HIR & Codegen Edge Cases
**Depends on:** 436

## Problem

The full Iterator helpers API (`map`, `filter`, `take`, `drop`, `flatMap`, `reduce`, `toArray`, `some`, `every`, `find`, `concat`) is not comprehensively exercised. Task 190 covers basic iterator helpers but not the full surface.

## HIR Coverage

- `Expr::Call` with `Expr::Member` callee for `Iterator.prototype.*` methods
- `Expr::Call` for `Iterator.from()`
- `Expr::Arrow` for callback arguments

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-iterator-helpers-full/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const arr = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
const iter = Iterator.from(arr);

const mapped = iter.map((x) => x * 2).take(5).toArray();
const filtered = Iterator.from(arr).filter((x) => x % 2 === 0).toArray();
const reduced = Iterator.from(arr).reduce((a, b) => a + b, 0);
const some = Iterator.from(arr).some((x) => x > 8);
const every = Iterator.from(arr).every((x) => x > 0);
const found = Iterator.from(arr).find((x) => x > 5);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Mapped: {mapped.join(', ')}</Text>
      <Text>Filtered: {filtered.join(', ')}</Text>
      <Text>Reduced: {reduced}</Text>
      <Text>Some > 8: {some ? 'yes' : 'no'}</Text>
      <Text>Every > 0: {every ? 'yes' : 'no'}</Text>
      <Text>Found > 5: {found}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-iterator-helpers-full/`
- [ ] Uses `map`, `filter`, `take`, `reduce`, `some`, `every`, `find`, `toArray`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
