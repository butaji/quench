# Task 406: `ink-sparse-array` Example — Sparse Arrays with Holes `[1, , 3]`

**Priority:** P2-Medium
**Phase:** 32 — Core Language + React + Runtime Edge Cases
**Depends on:** 405

## Problem

Sparse arrays (`[1, , 3]`) have holes that are not `undefined` but are absent properties. No existing Ink example explicitly exercises this edge case.

## HIR Coverage

- `Expr::Array` with `None` elements representing holes.
- The parser must preserve holes as distinct from `undefined`.

## Compile-Path Codegen

- `quote_codegen_exprs.inc` must emit compilable Rust for sparse arrays.
- Holes may map to `None` in generated Rust vectors or be skipped.

## Ink Example

```tsx
// examples/ink-sparse-array/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const sparse = [1, , 3, , 5];

const hasOne = 0 in sparse;
const hasHole = 1 in sparse;
const length = sparse.length;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Length: {length}</Text>
      <Text>Has 0: {hasOne ? 'yes' : 'no'}</Text>
      <Text>Has 1: {hasHole ? 'yes' : 'no'}</Text>
      <Text>Join: {sparse.join(',')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-sparse-array/`
- [ ] Uses sparse array `[1, , 3]`
- [ ] HIR `Expr::Array` with holes produces compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
