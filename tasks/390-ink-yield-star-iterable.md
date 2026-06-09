# Task 390: `ink-yield-star-iterable` Example — `yield*` with Arrays, Strings, Maps, Sets

**Priority:** P2-Medium
**Phase:** 31 — Advanced TS/TSX + React Edge Cases
**Depends on:** 389

## Problem

Generator `yield*` can delegate to any iterable, not just other generators. This includes arrays, strings, Maps, Sets, and custom iterables. No existing Ink example explicitly exercises `yield*` with diverse iterables.

## HIR Coverage

- `Expr::YieldFrom` (or `Expr::Yield` with delegation flag).
- The parser must handle `yield*` on any iterable expression.

## Compile-Path Codegen

- `quote_codegen_exprs.inc` must emit compilable Rust for `yield*` expressions.
- Delegation to non-generator iterables requires iterator protocol support.

## Ink Example

```tsx
// examples/ink-yield-star-iterable/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function* gen() {
  yield* [1, 2, 3];
  yield* 'ab';
  yield* new Map([['x', 10], ['y', 20]]).entries();
  yield* new Set([100, 200]);
}

const items: (string | number | [string, number])[] = [];
for (const item of gen()) {
  items.push(item);
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Items: {items.map(String).join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-yield-star-iterable/`
- [ ] Uses `yield*` with arrays, strings, Map entries, and Sets
- [ ] HIR `YieldFrom` produces compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
