# Task 400: `ink-array-flat-infinity` Example — Array.flat with Infinity Depth

**Priority:** P2-Medium
**Phase:** 32 — Core Language + React + Runtime Edge Cases
**Depends on:** 399

## Problem

`Array.prototype.flat(Infinity)` flattens arbitrarily nested arrays. No existing Ink example explicitly tests this edge case.

## HIR Coverage

- `Expr::Call` for `arr.flat(Infinity)`.
- `Expr::Ident` for `Infinity` global.

## Compile-Path Codegen

- `quote_codegen_exprs.inc` must emit compilable Rust for `Array.prototype.flat` with `Infinity` argument.

## Ink Example

```tsx
// examples/ink-array-flat-infinity/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const nested = [1, [2, [3, [4, [5]]]]];
const flat = nested.flat(Infinity);

export default function App() {
  return (
    <Box>
      <Text>Flat: {flat.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-array-flat-infinity/`
- [ ] Uses `Array.prototype.flat(Infinity)`
- [ ] HIR `Expr::Call` with `Infinity` produces compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
